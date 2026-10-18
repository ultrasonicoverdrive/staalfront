//! The wgpu side of the UI: one pipeline, the atlas texture, the minimap
//! texture, and a pass recorded on top of the finished scene.
//!
//! Usage per frame (see `sf_game`): build the UI with [`crate::Ui`], then
//! `renderer.render_with(|target| ui_renderer.render(target, &mut ui))`.

use bytemuck::{Pod, Zeroable};
use sf_render::FrameTarget;
use sf_render::wgpu::{self, util::DeviceExt};

use crate::atlas::WIDTH as ATLAS_WIDTH;
use crate::draw::{Texture, Vertex};
use crate::minimap::MinimapImage;
use crate::ui::Ui;

/// The UI shader, embedded at build time.
pub const EMBEDDED_SHADER: &str = include_str!("../../../assets/shaders/ui.wgsl");

const VERTEX_ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4];

/// Mirrors `Screen` in `ui.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ScreenUniform {
    size: [f32; 2],
    srgb_target: f32,
    _pad: f32,
}

/// A texture with its bind group.
struct Bound {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// Draws a [`Ui`] into a frame.
pub struct UiRenderer {
    pipeline: wgpu::RenderPipeline,
    screen_buffer: wgpu::Buffer,
    screen_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    srgb: bool,
    atlas: Option<Bound>,
    atlas_height: u32,
    atlas_revision: u64,
    minimap: Option<Bound>,
    vertices: wgpu::Buffer,
    vertex_capacity: u64,
    indices: wgpu::Buffer,
    index_capacity: u64,
    draw_calls: u32,
}

fn buffer(device: &wgpu::Device, usage: wgpu::BufferUsages, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ui buffer"),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl UiRenderer {
    /// Create the pipeline for a target of `format` (the renderer's colour
    /// format, [`sf_render::Renderer::color_format`]).
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui.wgsl"),
            source: wgpu::ShaderSource::Wgsl(EMBEDDED_SHADER.into()),
        });
        let screen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui screen layout"),
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
            label: Some("ui texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    // The vertex stage reads the texture size for the UVs.
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
            label: Some("ui pipeline layout"),
            bind_group_layouts: &[Some(&screen_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui pipeline"),
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
            label: Some("ui screen"),
            contents: bytemuck::bytes_of(&ScreenUniform {
                size: [1.0, 1.0],
                srgb_target: f32::from(u8::from(srgb)),
                _pad: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let screen_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui screen group"),
            layout: &screen_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ui sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            screen_buffer,
            screen_group,
            texture_layout,
            sampler,
            srgb,
            atlas: None,
            atlas_height: 0,
            atlas_revision: u64::MAX,
            minimap: None,
            vertices: buffer(device, wgpu::BufferUsages::VERTEX, 4096),
            vertex_capacity: 4096,
            indices: buffer(device, wgpu::BufferUsages::INDEX, 4096),
            index_capacity: 4096,
            draw_calls: 0,
        }
    }

    /// Draw calls of the last [`UiRenderer::render`] (one per texture batch).
    pub fn draw_calls(&self) -> u32 {
        self.draw_calls
    }

    fn bound(
        &self,
        device: &wgpu::Device,
        label: &str,
        width: u32,
        height: u32,
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
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Bound {
            texture,
            bind_group,
        }
    }

    /// Upload the baked terrain image shown by [`Ui::minimap`]. Call again
    /// when the map changes.
    pub fn set_minimap(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &MinimapImage,
    ) {
        let format = if self.srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let bound = self.bound(device, "ui minimap", image.width, image.height, format);
        queue.write_texture(
            bound.texture.as_image_copy(),
            &image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
        );
        self.minimap = Some(bound);
    }

    /// Record the UI pass on top of the frame's colour target.
    pub fn render(&mut self, target: &mut FrameTarget, ui: &mut Ui) {
        let (device, queue) = (target.device, target.queue);
        queue.write_buffer(
            &self.screen_buffer,
            0,
            bytemuck::bytes_of(&ScreenUniform {
                size: [target.size.0 as f32, target.size.1 as f32],
                srgb_target: f32::from(u8::from(self.srgb)),
                _pad: 0.0,
            }),
        );

        // Bring the atlas texture up to date: recreate it when the atlas grew,
        // re-upload it when glyphs were added since the last frame.
        let atlas_height = ui.atlas().height();
        if self.atlas.is_none() || self.atlas_height != atlas_height {
            self.atlas = Some(self.bound(
                device,
                "ui atlas",
                ATLAS_WIDTH,
                atlas_height,
                wgpu::TextureFormat::Rgba8Unorm,
            ));
            self.atlas_height = atlas_height;
            self.atlas_revision = u64::MAX;
        }
        if self.atlas_revision != ui.atlas().revision()
            && let Some(atlas) = &self.atlas
        {
            queue.write_texture(
                atlas.texture.as_image_copy(),
                ui.atlas().pixels(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ATLAS_WIDTH * 4),
                    rows_per_image: Some(atlas_height),
                },
                wgpu::Extent3d {
                    width: ATLAS_WIDTH,
                    height: atlas_height,
                    depth_or_array_layers: 1,
                },
            );
            self.atlas_revision = ui.atlas().revision();
        }

        self.draw_calls = 0;
        // The world layer (under the panels) and the panels share one vertex
        // buffer and one index buffer: the world layer first, the panels
        // after it, drawn with a base vertex so their indices need no
        // rewriting.
        let (under, list) = (ui.underlay(), ui.draw_list());
        if list.is_empty() && under.is_empty() {
            return;
        }
        let under_vbytes: &[u8] = bytemuck::cast_slice(&under.vertices);
        let under_ibytes: &[u8] = bytemuck::cast_slice(&under.indices);
        let list_vbytes: &[u8] = bytemuck::cast_slice(&list.vertices);
        let list_ibytes: &[u8] = bytemuck::cast_slice(&list.indices);
        let vlen = (under_vbytes.len() + list_vbytes.len()) as u64;
        let ilen = (under_ibytes.len() + list_ibytes.len()) as u64;
        if vlen > self.vertex_capacity {
            self.vertex_capacity = vlen.next_power_of_two();
            self.vertices = buffer(device, wgpu::BufferUsages::VERTEX, self.vertex_capacity);
        }
        if ilen > self.index_capacity {
            self.index_capacity = ilen.next_power_of_two();
            self.indices = buffer(device, wgpu::BufferUsages::INDEX, self.index_capacity);
        }
        queue.write_buffer(&self.vertices, 0, under_vbytes);
        queue.write_buffer(&self.vertices, under_vbytes.len() as u64, list_vbytes);
        queue.write_buffer(&self.indices, 0, under_ibytes);
        queue.write_buffer(&self.indices, under_ibytes.len() as u64, list_ibytes);

        let mut pass = target
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.screen_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..vlen));
        pass.set_index_buffer(self.indices.slice(..ilen), wgpu::IndexFormat::Uint32);
        let mut calls = 0;
        let parts = [
            (under, 0_i32, 0_u32),
            (
                list,
                under.vertices.len() as i32,
                under.indices.len() as u32,
            ),
        ];
        for (part, base_vertex, first_index) in parts {
            for batch in &part.batches {
                let bound = match batch.texture {
                    Texture::Atlas => self.atlas.as_ref(),
                    Texture::Minimap => self.minimap.as_ref(),
                };
                let Some(bound) = bound else { continue };
                pass.set_bind_group(1, &bound.bind_group, &[]);
                pass.draw_indexed(
                    first_index + batch.start..first_index + batch.end,
                    base_vertex,
                    0..1,
                );
                calls += 1;
            }
        }
        drop(pass);
        self.draw_calls = calls;
    }
}
