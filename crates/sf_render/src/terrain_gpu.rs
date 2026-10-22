//! Terrain on the GPU (WP1.7, WP1.8): one vertex buffer and one index buffer
//! for all chunks and levels, the material texture array, the per-cell
//! material index texture and the terrain pipeline. Used by
//! `Renderer::set_terrain`; the CPU side is [`crate::terrain`], the level
//! selection [`crate::lod`].

use wgpu::util::DeviceExt;

use crate::lod::{LodPlanner, TerrainConfig};
use crate::terrain::{self, MeshOptions, TEXTURE_SIZE, TILE_METERS, TerrainVertex};
use crate::terrain_look::{TerrainLooks, terrain_surface};

/// The terrain shader, embedded at build time (no hot-reload yet).
pub const EMBEDDED_TERRAIN_SHADER: &str = include_str!("../../../assets/shaders/terrain.wgsl");

const VERTEX_ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32];

/// Mirrors `TerrainParams` in `terrain.wgsl` (56 bytes, no vector members, so
/// the WGSL layout is plain 4-byte packing plus the 8-byte `index_size`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TerrainParams {
    /// Size of the cell index texture in texels.
    index_size: [i32; 2],
    tile_meters: f32,
    /// Debug flags, bit 0: tint chunks by their level of detail.
    flags: u32,
    /// Opacity of the gameplay cell grid; 0 = off.
    grid_alpha: f32,
    /// Metres between the grid lines: one gameplay cell (B12).
    cell_meters: f32,
    /// Metres per index texel (4 with terrain types, 1 without).
    index_meters: f32,
    /// Half the width of the blend ramp in texels.
    ramp: f32,
    /// Wobble of the borders in texels.
    wobble: f32,
    shore_amount: f32,
    shore_color: [f32; 3],
    _pad: f32,
}

/// `flags` bit: tint chunks by their level of detail.
const FLAG_LOD_TINT: u32 = 1;

/// Everything `Renderer::set_terrain` creates.
pub struct TerrainGpu {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
    /// All chunks at all levels (see [`terrain::PackedTerrain`]).
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    /// The `TerrainParams` uniform, rewritten by [`TerrainGpu::write_debug_view`].
    params: wgpu::Buffer,
    /// The params without the debug view (what the map decided).
    base_params: TerrainParams,
    pub config: TerrainConfig,
    pub planner: LodPlanner,
    /// Debug: fixed level per chunk instead of the automatic selection.
    pub level_override: Option<Vec<u8>>,
}

impl TerrainGpu {
    /// Update the debug visualisation flags in the terrain uniform.
    pub fn write_debug_view(&self, queue: &wgpu::Queue, view: crate::renderer::DebugView) {
        let params = TerrainParams {
            flags: if view.lod_tint { FLAG_LOD_TINT } else { 0 },
            grid_alpha: view.grid_alpha.clamp(0.0, 1.0),
            ..self.base_params
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
    }
}

/// What a terrain is built from: the map, the table of terrain types its layer
/// uses and the looks of those types.
pub struct TerrainSource<'a> {
    pub map: &'a sf_data::map::Map,
    pub types: &'a sf_data::terrain::TerrainTypes,
    pub looks: &'a TerrainLooks,
}

pub fn build(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    globals_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    source: &TerrainSource<'_>,
    config: &TerrainConfig,
) -> TerrainGpu {
    let TerrainSource { map, types, looks } = *source;
    // Chunk meshes of all levels in one vertex and one index buffer.
    let mesh = terrain::build_mesh_with(
        map,
        MeshOptions {
            skirts: config.seams,
        },
    );
    let packed = terrain::pack(&mesh);
    let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("terrain vertices"),
        contents: bytemuck::cast_slice(&packed.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("terrain indices"),
        contents: bytemuck::cast_slice(&packed.indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let planner = LodPlanner::new(&mesh, packed.ranges);

    // Material texture array with a full mip chain: one layer per terrain
    // type (WP2.2), or per map material without a terrain layer.
    let surface = terrain_surface(map, types, looks);
    let layers = surface.layers.len() as u32;
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
    for (layer, level0) in surface.layers.iter().enumerate() {
        let levels = terrain::mip_chain(level0.clone(), TEXTURE_SIZE);
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

    // Layer and style per cell: one texel per gameplay cell (per 1 m sample
    // without a terrain layer).
    let cell_extent = wgpu::Extent3d {
        width: surface.size.0,
        height: surface.size.1,
        depth_or_array_layers: 1,
    };
    let cell_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terrain cell index"),
        size: cell_extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rg8Uint,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        cell_texture.as_image_copy(),
        &surface.index,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(surface.size.0 * 2),
            rows_per_image: Some(surface.size.1),
        },
        cell_extent,
    );
    let cell_view = cell_texture.create_view(&wgpu::TextureViewDescriptor::default());

    let base_params = TerrainParams {
        index_size: [surface.size.0 as i32, surface.size.1 as i32],
        tile_meters: TILE_METERS,
        flags: 0,
        grid_alpha: 0.0,
        cell_meters: sf_math::render_bridge::METRES_PER_CELL,
        index_meters: surface.index_meters,
        ramp: surface.ramp,
        wobble: surface.wobble,
        shore_amount: surface.shore_amount,
        shore_color: surface.shore,
        _pad: 0.0,
    };
    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("terrain params"),
        contents: bytemuck::bytes_of(&base_params),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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
                array_stride: std::mem::size_of::<TerrainVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &VERTEX_ATTRS,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            // Back faces are culled: the surface faces up and the skirts face
            // outward. A skirt that covers a gap is the higher chunk's, seen
            // from the lower side, which is its front (tested by rendering the
            // seams from both sides).
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
        vertices,
        indices,
        params,
        base_params,
        config: *config,
        planner,
        level_override: None,
    }
}
