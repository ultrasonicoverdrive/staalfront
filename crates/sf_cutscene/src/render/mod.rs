//! Drawing a [`SceneState`]: the cartoon renderer.
//!
//! [`CutsceneRenderer`] draws one frame of a scene into the frame the
//! engine's [`sf_render::Renderer`] is rendering, through the renderer's frame
//! hook (`render_with` for a window, `render_to_image_with` for a screenshot).
//! It adds one render pass that clears the target to black and draws the
//! picture; nothing in `sf_render`, `sf_ui` or `sf_game` is changed.
//!
//! * [`plan`]: from a [`SceneState`] to quads, with no GPU (what is drawn
//!   where, in which order; unit-tested).
//! * [`text`]: subtitle wrapping and the box geometry, on the UI's font.
//! * [`images`]: PNG pictures for image parts and layers.
//! * this file: the wgpu pipeline (`assets/shaders/cutscene.wgsl`), textures
//!   and buffers.
//!
//! Placeholder art (rectangles, rounded rectangles, circles) is drawn from a
//! signed distance in the fragment shader, so cutout parts rotate with smooth
//! edges. Pictures that cannot be loaded are drawn as a magenta box, never as
//! a hole.
//!
//! Usage per frame:
//!
//! ```ignore
//! player.update(now_ms, &mut sink);
//! let state = player.sample();
//! renderer.render_with(|target| cutscenes.render(target, player.scene(), &state, None))?;
//! ```

pub mod images;
pub mod plan;
pub mod text;

use bytemuck::{Pod, Zeroable};
use sf_render::FrameTarget;
use sf_render::wgpu::{self, util::DeviceExt};
use sf_ui::atlas::WIDTH as ATLAS_WIDTH;

use crate::model::Scene;
use crate::sample::SceneState;

pub use images::{ImageStore, Picture};
pub use plan::{FrameLayout, FramePlan, Hud, Rect, Resources, Vertex, fit_viewport, view_affine};
pub use text::{SubtitleLayout, TextEngine, layout_subtitle, subtitle_font_px};

/// The shader, embedded at build time.
pub const EMBEDDED_SHADER: &str = include_str!("../../../../assets/shaders/cutscene.wgsl");

const VERTEX_ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x4, 4 => Float32, 5 => Unorm8x4
];

/// Mirrors `Screen` in `cutscene.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ScreenUniform {
    size: [f32; 2],
    srgb_target: f32,
    _pad: f32,
}

struct Bound {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// Draws scenes. Create one per render target format and keep it: it holds the
/// pipeline, the glyph atlas and every picture loaded so far.
pub struct CutsceneRenderer {
    pipeline: wgpu::RenderPipeline,
    screen_buffer: wgpu::Buffer,
    screen_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    srgb: bool,
    white: Bound,
    glyphs: Option<Bound>,
    glyph_height: u32,
    glyph_revision: u64,
    /// GPU copies of the pictures, by index in `res.images`.
    pictures: Vec<Option<Bound>>,
    vertices: wgpu::Buffer,
    vertex_capacity: u64,
    indices: wgpu::Buffer,
    index_capacity: u64,
    res: Resources,
    layout: Option<FrameLayout>,
    draw_calls: u32,
}

fn buffer(device: &wgpu::Device, usage: wgpu::BufferUsages, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("cutscene buffer"),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn bound_texture(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    label: &str,
    (width, height): (u32, u32),
    format: wgpu::TextureFormat,
) -> Bound {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    Bound {
        texture,
        bind_group,
    }
}

impl CutsceneRenderer {
    /// A renderer for targets of `format` (`Renderer::color_format()`), reading
    /// pictures from `asset_root` (`assets/`).
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        asset_root: impl Into<std::path::PathBuf>,
    ) -> CutsceneRenderer {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cutscene.wgsl"),
            source: wgpu::ShaderSource::Wgsl(EMBEDDED_SHADER.into()),
        });
        let screen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cutscene screen layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cutscene texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cutscene pipeline layout"),
            bind_group_layouts: &[Some(&screen_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cutscene pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &VERTEX_ATTRS,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                // Mirrored characters flip the winding.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
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
        let srgb = format.is_srgb();
        let screen_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cutscene screen"),
            contents: bytemuck::bytes_of(&ScreenUniform {
                size: [1.0, 1.0],
                srgb_target: f32::from(u8::from(srgb)),
                _pad: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let screen_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cutscene screen group"),
            layout: &screen_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cutscene sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = bound_texture(
            device,
            &texture_layout,
            &sampler,
            "cutscene white",
            (1, 1),
            wgpu::TextureFormat::Rgba8Unorm,
        );
        queue.write_texture(
            white.texture.as_image_copy(),
            &[255, 255, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        CutsceneRenderer {
            pipeline,
            screen_buffer,
            screen_group,
            texture_layout,
            sampler,
            srgb,
            white,
            glyphs: None,
            glyph_height: 0,
            glyph_revision: u64::MAX,
            pictures: Vec::new(),
            vertices: buffer(device, wgpu::BufferUsages::VERTEX, 4096),
            vertex_capacity: 4096,
            indices: buffer(device, wgpu::BufferUsages::INDEX, 4096),
            index_capacity: 4096,
            res: Resources::new(asset_root),
            layout: None,
            draw_calls: 0,
        }
    }

    fn bound(
        &self,
        device: &wgpu::Device,
        label: &str,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Bound {
        bound_texture(
            device,
            &self.texture_layout,
            &self.sampler,
            label,
            size,
            format,
        )
    }

    /// Pictures that were asked for and could not be loaded, with the reason.
    pub fn failed_pictures(&self) -> Vec<(&str, &str)> {
        self.res.images.failures()
    }

    /// Where the last frame put things (the picture, the subtitle box).
    pub fn layout(&self) -> Option<&FrameLayout> {
        self.layout.as_ref()
    }

    /// Draw calls of the last frame.
    pub fn draw_calls(&self) -> u32 {
        self.draw_calls
    }

    /// The quads of a frame without drawing them (tests, tools).
    pub fn plan(
        &mut self,
        scene: &Scene,
        state: &SceneState<'_>,
        target: (u32, u32),
        hud: Option<&Hud>,
    ) -> FramePlan {
        FramePlan::build(scene.stage, state, target, &mut self.res, hud)
    }

    /// Record the cutscene pass on `target`: clear to black, draw the frame of
    /// `state`. `hud` adds the viewer's progress bar.
    pub fn render(
        &mut self,
        target: &mut FrameTarget,
        scene: &Scene,
        state: &SceneState<'_>,
        hud: Option<&Hud>,
    ) {
        let (device, queue) = (target.device, target.queue);
        let plan = self.plan(scene, state, target.size, hud);

        queue.write_buffer(
            &self.screen_buffer,
            0,
            bytemuck::bytes_of(&ScreenUniform {
                size: [target.size.0 as f32, target.size.1 as f32],
                srgb_target: f32::from(u8::from(self.srgb)),
                _pad: 0.0,
            }),
        );
        self.sync_glyphs(device, queue);
        self.sync_pictures(device, queue, &plan);

        self.draw_calls = 0;
        let vbytes: &[u8] = bytemuck::cast_slice(&plan.vertices);
        let ibytes: &[u8] = bytemuck::cast_slice(&plan.indices);
        if !vbytes.is_empty() {
            if vbytes.len() as u64 > self.vertex_capacity {
                self.vertex_capacity = (vbytes.len() as u64).next_power_of_two();
                self.vertices = buffer(device, wgpu::BufferUsages::VERTEX, self.vertex_capacity);
            }
            if ibytes.len() as u64 > self.index_capacity {
                self.index_capacity = (ibytes.len() as u64).next_power_of_two();
                self.indices = buffer(device, wgpu::BufferUsages::INDEX, self.index_capacity);
            }
            queue.write_buffer(&self.vertices, 0, vbytes);
            queue.write_buffer(&self.indices, 0, ibytes);
        }

        let mut pass = target
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cutscene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        if !vbytes.is_empty() {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.screen_group, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..vbytes.len() as u64));
            pass.set_index_buffer(
                self.indices.slice(..ibytes.len() as u64),
                wgpu::IndexFormat::Uint32,
            );
            let mut calls = 0;
            for batch in &plan.batches {
                let bound = match batch.tex {
                    plan::TexId::White => Some(&self.white),
                    plan::TexId::Glyphs => self.glyphs.as_ref(),
                    plan::TexId::Image(i) => self.pictures.get(i).and_then(Option::as_ref),
                };
                let Some(bound) = bound else { continue };
                let s = batch.scissor;
                if s.w == 0 || s.h == 0 {
                    continue;
                }
                pass.set_scissor_rect(s.x, s.y, s.w, s.h);
                pass.set_bind_group(1, &bound.bind_group, &[]);
                pass.draw_indexed(batch.start..batch.end, 0, 0..1);
                calls += 1;
            }
            self.draw_calls = calls;
        }
        drop(pass);
        self.layout = Some(plan.layout);
    }

    /// Bring the glyph atlas texture up to date.
    fn sync_glyphs(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let atlas = &self.res.text.atlas;
        let height = atlas.height();
        if self.glyphs.is_none() || self.glyph_height != height {
            self.glyphs = Some(self.bound(
                device,
                "cutscene glyphs",
                (ATLAS_WIDTH, height),
                wgpu::TextureFormat::Rgba8Unorm,
            ));
            self.glyph_height = height;
            self.glyph_revision = u64::MAX;
        }
        if self.glyph_revision != atlas.revision()
            && let Some(g) = &self.glyphs
        {
            queue.write_texture(
                g.texture.as_image_copy(),
                atlas.pixels(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ATLAS_WIDTH * 4),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width: ATLAS_WIDTH,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            self.glyph_revision = atlas.revision();
        }
    }

    /// Upload the pictures a plan uses that are not on the GPU yet.
    fn sync_pictures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, plan: &FramePlan) {
        let format = if self.srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        for batch in &plan.batches {
            let plan::TexId::Image(i) = batch.tex else {
                continue;
            };
            if self.pictures.len() <= i {
                self.pictures.resize_with(i + 1, || None);
            }
            if self.pictures[i].is_some() {
                continue;
            }
            let pic = self.res.images.picture(i);
            let bound = self.bound(device, "cutscene picture", (pic.width, pic.height), format);
            queue.write_texture(
                bound.texture.as_image_copy(),
                &pic.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pic.width * 4),
                    rows_per_image: Some(pic.height),
                },
                wgpu::Extent3d {
                    width: pic.width,
                    height: pic.height,
                    depth_or_array_layers: 1,
                },
            );
            self.pictures[i] = Some(bound);
        }
    }
}
