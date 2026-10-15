//! Terrain on the GPU (WP1.7): per-chunk buffers, the material texture array,
//! the per-cell material index texture and the terrain pipeline. Used by
//! `Renderer::set_terrain`; the CPU side is [`crate::terrain`].

use wgpu::util::DeviceExt;

use crate::mesh::Vertex;
use crate::terrain::{self, TEXTURE_SIZE, TILE_METERS};

/// The terrain shader, embedded at build time (no hot-reload yet).
pub const EMBEDDED_TERRAIN_SHADER: &str = include_str!("../../../assets/shaders/terrain.wgsl");

const VERTEX_ATTRS: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

/// One terrain chunk on the GPU: its own vertex and index buffer.
pub struct GpuChunk {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub index_count: u32,
}

/// Mirrors `TerrainParams` in `terrain.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TerrainParams {
    map_cells: [i32; 2],
    tile_meters: f32,
    _pad: f32,
}

/// Everything `Renderer::set_terrain` creates.
pub struct TerrainGpu {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
    pub chunks: Vec<GpuChunk>,
}

pub fn build(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    globals_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    map: &sf_data::map::Map,
) -> TerrainGpu {
    // Chunk buffers.
    let mesh = terrain::build_mesh(map);
    let chunks = mesh
        .chunks
        .iter()
        .map(|c| GpuChunk {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("terrain chunk vertices"),
                contents: bytemuck::cast_slice(&c.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("terrain chunk indices"),
                contents: bytemuck::cast_slice(&c.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            index_count: c.indices.len() as u32,
        })
        .collect();

    // Material texture array with a full mip chain.
    let layers = map.materials().len() as u32;
    let mip_levels = TEXTURE_SIZE.ilog2() + 1;
    let array = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terrain materials"),
        size: wgpu::Extent3d {
            width: TEXTURE_SIZE,
            height: TEXTURE_SIZE,
            depth_or_array_layers: layers,
        },
        mip_level_count: mip_levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (layer, name) in map.materials().iter().enumerate() {
        let levels = terrain::mip_chain(terrain::material_texture(name), TEXTURE_SIZE);
        for (level, texels) in levels.iter().enumerate() {
            let size = (TEXTURE_SIZE >> level).max(1);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &array,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                texels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size * 4),
                    rows_per_image: Some(size),
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
    let array_view = array.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("terrain material sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    });

    // Material index per cell.
    let cell_extent = wgpu::Extent3d {
        width: map.width(),
        height: map.height(),
        depth_or_array_layers: 1,
    };
    let cell_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terrain cell materials"),
        size: cell_extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Uint,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        cell_texture.as_image_copy(),
        map.cells(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(map.width()),
            rows_per_image: Some(map.height()),
        },
        cell_extent,
    );
    let cell_view = cell_texture.create_view(&wgpu::TextureViewDescriptor::default());

    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("terrain params"),
        contents: bytemuck::bytes_of(&TerrainParams {
            map_cells: [map.width() as i32, map.height() as i32],
            tile_meters: TILE_METERS,
            _pad: 0.0,
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("terrain layout"),
        entries: &[
            entry(
                0,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
            ),
            entry(
                1,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ),
            entry(
                2,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
            entry(
                3,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("terrain bind group"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&array_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&cell_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: params.as_entire_binding(),
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("terrain pipeline layout"),
        bind_group_layouts: &[Some(globals_layout), Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("terrain.wgsl"),
        source: wgpu::ShaderSource::Wgsl(EMBEDDED_TERRAIN_SHADER.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("terrain pipeline"),
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
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
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
    TerrainGpu {
        pipeline,
        bind_group,
        chunks,
    }
}
