//! The forward renderer: window surface or offscreen texture, depth buffer,
//! one directional light + ambient, the built-in test scene.

use std::ops::Range;

use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::gpu::{Gpu, GpuOptions, RenderError};
use crate::image::Rgba8;
use crate::mesh::{MeshData, Vertex};
use crate::scene::{
    Globals, Instance, Lighting, MeshKind, PlaceholderCamera, SceneObject, test_scene,
};

/// The shader embedded at build time. Release builds always use this; debug
/// builds start from it and may hot-reload from disk (see
/// [`Renderer::watch_shader_file`]).
pub const EMBEDDED_SHADER: &str = include_str!("../../../assets/shaders/scene.wgsl");

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
    camera: PlaceholderCamera,
    /// When set, replaces the placeholder camera (see [`Renderer::set_view_proj`]).
    view_proj: Option<Mat4>,
    lighting: Lighting,
    #[cfg(debug_assertions)]
    watch: Option<crate::shader_watch::ShaderWatch>,
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
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&gpu.device, &config);
        let format = config.format;
        Ok(Self::build(
            gpu,
            Target::Window { surface, config },
            format,
            (width, height),
        ))
    }

    /// Renderer drawing into an offscreen texture (screenshots, tests).
    pub fn headless(width: u32, height: u32, options: &GpuOptions) -> Result<Self, RenderError> {
        let gpu = Gpu::headless(options)?;
        let (width, height) = (width.max(1), height.max(1));
        let texture = offscreen_texture(&gpu.device, width, height);
        Ok(Self::build(
            gpu,
            Target::Offscreen { texture },
            OFFSCREEN_FORMAT,
            (width, height),
        ))
    }

    fn build(gpu: Gpu, target: Target, format: wgpu::TextureFormat, size: (u32, u32)) -> Self {
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

        let meshes = MeshKind::ALL
            .iter()
            .map(|kind| {
                let data = match kind {
                    MeshKind::Plane => MeshData::plane(),
                    MeshKind::Cube => MeshData::cube(),
                    MeshKind::Pyramid => MeshData::pyramid(),
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
            camera,
            view_proj: None,
            lighting,
            #[cfg(debug_assertions)]
            watch: None,
        }
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

    /// Draw one frame: present to the window, or fill the offscreen texture.
    pub fn render(&mut self) -> Result<(), RenderError> {
        let _ = self.poll_shader_reload();
        self.write_globals();
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
                match surface.get_current_texture() {
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

        self.draw(&mut encoder, &view);
        self.gpu.queue.submit([encoder.finish()]);
        if let Some(frame) = frame {
            self.gpu.queue.present(frame);
        }
        Ok(())
    }

    /// Render and read the offscreen texture back as sRGB RGBA8.
    pub fn render_to_image(&mut self) -> Result<Rgba8, RenderError> {
        let Target::Offscreen { .. } = self.target else {
            return Err(RenderError::Readback(
                "render_to_image needs a headless renderer".into(),
            ));
        };
        self.write_globals();
        let (width, height) = self.size;
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("screenshot"),
            });
        let Target::Offscreen { texture } = &self.target else {
            unreachable!("checked above");
        };
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.draw(&mut encoder, &view);

        let unpadded = width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let readback = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded) * u64::from(height),
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
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.gpu.queue.submit([encoder.finish()]);

        let (tx, rx) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.gpu
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        rx.recv()
            .map_err(|e| RenderError::Readback(e.to_string()))?
            .map_err(|e| RenderError::Readback(e.to_string()))?;

        let mapped = readback
            .slice(..)
            .get_mapped_range()
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        let mut pixels = Vec::with_capacity(unpadded as usize * height as usize);
        for row in mapped.chunks_exact(padded as usize) {
            pixels.extend_from_slice(&row[..unpadded as usize]);
        }
        drop(mapped);
        readback.unmap();
        Ok(Rgba8::new(width, height, pixels))
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

    /// Show a terrain: builds the chunk meshes, the material texture array
    /// and the material index texture from `map` and draws it before the
    /// scene objects. Replaces a previous terrain. Call on map changes, not
    /// every frame.
    pub fn set_terrain(&mut self, map: &sf_data::map::Map) {
        self.terrain = Some(crate::terrain_gpu::build(
            &self.gpu.device,
            &self.gpu.queue,
            &self.globals_layout,
            self.format,
            DEPTH_FORMAT,
            map,
        ));
    }

    /// Remove the terrain.
    pub fn clear_terrain(&mut self) {
        self.terrain = None;
    }

    /// Number of terrain chunks drawn per frame (0 without terrain).
    pub fn terrain_chunk_count(&self) -> usize {
        self.terrain.as_ref().map_or(0, |t| t.chunks.len())
    }

    fn write_globals(&self) {
        let view_proj = self
            .view_proj
            .unwrap_or_else(|| self.camera.view_proj(aspect(self.size)));
        let globals = Globals::new(view_proj, &self.lighting);
        self.gpu
            .queue
            .write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
    }

    fn draw(&self, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
        });
        if let Some(t) = &self.terrain {
            pass.set_pipeline(&t.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_bind_group(1, &t.bind_group, &[]);
            for chunk in &t.chunks {
                pass.set_vertex_buffer(0, chunk.vertices.slice(..));
                pass.set_index_buffer(chunk.indices.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..chunk.index_count, 0, 0..1);
            }
        }
        if self.draws.is_empty() {
            return; // an empty instance buffer cannot be bound
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(1, self.instances.slice(..));
        for (mesh, range) in &self.draws {
            let mesh = &self.meshes[*mesh];
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, range.clone());
        }
    }
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

const VERTEX_ATTRS: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4
];

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
                Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &INSTANCE_ATTRS,
                }),
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
