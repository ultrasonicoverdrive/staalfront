//! Cell grid overlay (WP2.4): every 4 m gameplay cell tinted by its terrain
//! type, with the occupancy on top. A debug / planning view, off by default.
//!
//! # What a cell shows
//!
//! Each cell has one [`CellState`], highest first:
//!
//! * [`CellState::Structure`]: the sim's occupancy holds a structure on the
//!   cell (`World::occupant_at`; a structure with a footprint covers all its
//!   cells). This is the only occupancy the simulation keeps per cell.
//! * [`CellState::Units`]: at least one entity that is not a structure has its
//!   current position in the cell. The sim does **not** track this per cell
//!   (units have bodies, not cells, see `sf_sim::terrain`); the presentation
//!   derives it from the snapshot's positions, so it is "where units are", not
//!   "what blocks".
//! * [`CellState::Impassable`]: the terrain type is not open ground: at least
//!   one ground movement class (foot, wheeled, tracked) cannot enter it. The
//!   sim's `TerrainFlags::ground` is false (it is true only when every ground
//!   class can enter), so water is impassable even though boats sail it.
//! * [`CellState::Open`]: none of the above.
//!
//! The colours are data (`assets/config/cell_overlay.ron`).
//!
//! # Cost
//!
//! One texel per cell (`Rgba8Uint`: the colour and the state), written by
//! [`CellLayer::update`] at most once per sim tick; the upload covers the span
//! from the first changed row to the last ([`upload_span`]). Drawing is one
//! more pass over the terrain draws the level of detail already selected (one
//! call per run of chunks), with a shader that reads one texel per fragment.
//! When the overlay is off nothing is rebuilt, uploaded or drawn; the layer,
//! texture and pipeline are built at start-up regardless.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The shipped colours, compiled in like the other render settings.
pub const EMBEDDED_CELL_OVERLAY: &str = include_str!("../../../assets/config/cell_overlay.ron");

/// The overlay shader, embedded at build time.
pub const EMBEDDED_CELL_OVERLAY_SHADER: &str =
    include_str!("../../../assets/shaders/cell_overlay.wgsl");

/// What a cell shows, besides its type colour. The numbers are what the
/// shader reads from the texel's alpha byte (`cell_overlay.wgsl`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum CellState {
    Open = 0,
    Impassable = 1,
    Units = 2,
    Structure = 3,
}

/// The RON file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellOverlayStyle {
    /// Opacity of the type tint, in percent.
    pub fill_percent: u8,
    /// sRGB colour per terrain type name.
    pub types: BTreeMap<String, [u8; 3]>,
    /// Cells with units in them.
    pub units: [u8; 3],
    /// Cells a structure occupies.
    pub structure: [u8; 3],
}

impl CellOverlayStyle {
    /// The shipped style.
    pub fn embedded() -> CellOverlayStyle {
        Self::from_ron(EMBEDDED_CELL_OVERLAY).expect("assets/config/cell_overlay.ron is valid")
    }

    pub fn from_ron(text: &str) -> Result<CellOverlayStyle, String> {
        let style: CellOverlayStyle =
            ron::from_str(text).map_err(|e| format!("cell_overlay.ron: {e}"))?;
        if style.fill_percent > 100 {
            return Err("cell_overlay.ron: fill_percent is more than 100".into());
        }
        Ok(style)
    }

    /// The colour of each terrain type id: every type of `types` must have a
    /// colour and every colour must name a type.
    pub fn palette(&self, types: &sf_data::terrain::TerrainTypes) -> Result<CellPalette, String> {
        let mut by_id = [None; 256];
        for t in types.iter() {
            let rgb = self.types.get(&t.name).ok_or_else(|| {
                format!("cell_overlay.ron: no colour for terrain type {}", t.name)
            })?;
            by_id[usize::from(t.id)] = Some(*rgb);
        }
        if let Some(name) = self.types.keys().find(|n| types.id_of(n).is_none()) {
            return Err(format!("cell_overlay.ron: unknown terrain type {name}"));
        }
        Ok(CellPalette {
            by_id,
            units: self.units,
            structure: self.structure,
            fill_percent: self.fill_percent,
        })
    }
}

/// The overlay colours resolved to terrain type ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellPalette {
    by_id: [Option<[u8; 3]>; 256],
    pub units: [u8; 3],
    pub structure: [u8; 3],
    pub fill_percent: u8,
}

/// The colour of an id that has none (cannot happen on a validated map).
const UNKNOWN: [u8; 3] = [255, 0, 255];

impl CellPalette {
    /// The type colour of `id`.
    pub fn type_colour(&self, id: u8) -> [u8; 3] {
        self.by_id[usize::from(id)].unwrap_or(UNKNOWN)
    }

    /// The texel of a cell: its colour (sRGB) and its state.
    pub fn texel(&self, id: u8, state: CellState) -> [u8; 4] {
        let [r, g, b] = match state {
            CellState::Open | CellState::Impassable => self.type_colour(id),
            CellState::Units => self.units,
            CellState::Structure => self.structure,
        };
        [r, g, b, state as u8]
    }
}

/// The state of a cell from what is known about it.
pub fn cell_state(impassable: bool, units: bool, structure: bool) -> CellState {
    if structure {
        CellState::Structure
    } else if units {
        CellState::Units
    } else if impassable {
        CellState::Impassable
    } else {
        CellState::Open
    }
}

/// The CPU side of the overlay: the terrain of each cell (fixed) and the
/// texels as last uploaded.
pub struct CellLayer {
    width: u32,
    height: u32,
    palette: CellPalette,
    /// Type id per cell, row-major.
    types: Vec<u8>,
    /// Whether each type id is impassable for ground units.
    impassable: [bool; 256],
    texels: Vec<[u8; 4]>,
    /// Scratch: occupancy marks of the update in progress (bit 0 units, bit 1
    /// structure).
    marks: Vec<u8>,
}

impl CellLayer {
    /// A layer for a `width` x `height` cell map; `types` holds the type id of
    /// every cell, row-major; `impassable(id)` says whether ground units are
    /// kept off a type.
    pub fn new(
        width: u32,
        height: u32,
        types: Vec<u8>,
        impassable: impl Fn(u8) -> bool,
        palette: CellPalette,
    ) -> CellLayer {
        assert_eq!(types.len(), width as usize * height as usize);
        let impassable = std::array::from_fn(|i| impassable(i as u8));
        let mut layer = CellLayer {
            width,
            height,
            palette,
            texels: vec![[0; 4]; types.len()],
            marks: vec![0; types.len()],
            types,
            impassable,
        };
        layer.update(std::iter::empty(), std::iter::empty());
        layer
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Row-major texels, 4 bytes per cell.
    pub fn texels(&self) -> &[[u8; 4]] {
        &self.texels
    }

    /// The texel of cell (`x`, `y`).
    pub fn texel(&self, x: u32, y: u32) -> [u8; 4] {
        self.texels[(y * self.width + x) as usize]
    }

    fn index(&self, (x, y): (u32, u32)) -> Option<usize> {
        (x < self.width && y < self.height).then(|| (y * self.width + x) as usize)
    }

    /// Recompute every cell from the cells that hold units and the cells
    /// structures occupy (out-of-map cells are ignored). Returns the rows
    /// that changed, `first..end`, or `None` if nothing did.
    pub fn update(
        &mut self,
        units: impl IntoIterator<Item = (u32, u32)>,
        structures: impl IntoIterator<Item = (u32, u32)>,
    ) -> Option<std::ops::Range<u32>> {
        self.marks.fill(0);
        for cell in units {
            if let Some(i) = self.index(cell) {
                self.marks[i] |= 1;
            }
        }
        for cell in structures {
            if let Some(i) = self.index(cell) {
                self.marks[i] |= 2;
            }
        }
        let mut rows: Option<std::ops::Range<u32>> = None;
        let width = self.width as usize;
        for (i, (texel, (&id, &mark))) in self
            .texels
            .iter_mut()
            .zip(self.types.iter().zip(&self.marks))
            .enumerate()
        {
            let state = cell_state(
                self.impassable[usize::from(id)],
                mark & 1 != 0,
                mark & 2 != 0,
            );
            let new = self.palette.texel(id, state);
            if *texel != new {
                *texel = new;
                let row = (i / width) as u32;
                rows = Some(match rows {
                    None => row..row + 1,
                    Some(r) => r.start.min(row)..r.end.max(row + 1),
                });
            }
        }
        rows
    }
}

/// What [`CellOverlayGpu::upload`] writes for the changed `rows` of a layer
/// of `(width, height)` cells: the rows clamped to the map, and the texel
/// indices they cover. `None` when nothing is left to upload.
pub fn upload_span(
    (width, height): (u32, u32),
    rows: std::ops::Range<u32>,
) -> Option<(std::ops::Range<u32>, std::ops::Range<usize>)> {
    let rows = rows.start.min(height)..rows.end.min(height);
    if rows.is_empty() {
        return None;
    }
    let texels = (rows.start * width) as usize..(rows.end * width) as usize;
    Some((rows, texels))
}

/// Mirrors `OverlayParams` in `cell_overlay.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct OverlayParams {
    map_cells: [i32; 2],
    cell_meters: f32,
    fill: f32,
}

/// The GPU side: texture, uniform, pipeline.
pub(crate) struct CellOverlayGpu {
    pub layer: CellLayer,
    texture: wgpu::Texture,
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
}

impl CellOverlayGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        globals_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        layer: CellLayer,
    ) -> CellOverlayGpu {
        use wgpu::util::DeviceExt;
        let (width, height) = layer.size();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cell overlay"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cell overlay params"),
            contents: bytemuck::bytes_of(&OverlayParams {
                map_cells: [width as i32, height as i32],
                cell_meters: sf_math::render_bridge::METRES_PER_CELL,
                fill: f32::from(layer.palette.fill_percent) / 100.0,
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
            label: Some("cell overlay layout"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cell overlay bind group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cell overlay pipeline layout"),
            bind_group_layouts: &[Some(globals_layout), Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cell_overlay.wgsl"),
            source: wgpu::ShaderSource::Wgsl(EMBEDDED_CELL_OVERLAY_SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cell overlay pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::terrain::TerrainVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &crate::terrain_gpu::VERTEX_ATTRS,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            // Drawn over the terrain it was lifted from: tested, not written,
            // so the units drawn after it cover it.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
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
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let gpu = CellOverlayGpu {
            layer,
            texture,
            pipeline,
            bind_group,
        };
        gpu.upload(queue, 0..height);
        gpu
    }

    /// Upload the texels of `rows` (see [`upload_span`]).
    pub fn upload(&self, queue: &wgpu::Queue, rows: std::ops::Range<u32>) {
        let (width, _) = self.layer.size();
        let Some((rows, texels)) = upload_span(self.layer.size(), rows) else {
            return;
        };
        let (start, end) = (texels.start, texels.end);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: rows.start,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&self.layer.texels()[start..end]),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(rows.end - rows.start),
            },
            wgpu::Extent3d {
                width,
                height: rows.end - rows.start,
                depth_or_array_layers: 1,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_data::terrain::TerrainTypes;

    fn palette() -> (TerrainTypes, CellPalette) {
        let types = TerrainTypes::embedded();
        let palette = CellOverlayStyle::embedded()
            .palette(&types)
            .expect("the shipped colours cover the shipped types");
        (types, palette)
    }

    #[test]
    fn every_shipped_type_has_its_own_colour() {
        let (types, palette) = palette();
        let mut seen: Vec<[u8; 3]> = types.iter().map(|t| palette.type_colour(t.id)).collect();
        seen.push(palette.units);
        seen.push(palette.structure);
        let n = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), n, "two overlay colours are the same");
    }

    #[test]
    fn a_missing_or_unknown_type_name_is_refused() {
        let types = TerrainTypes::embedded();
        let mut style = CellOverlayStyle::embedded();
        style.types.remove("ore");
        assert!(style.palette(&types).unwrap_err().contains("ore"));
        let mut style = CellOverlayStyle::embedded();
        style.types.insert("lava".into(), [1, 2, 3]);
        assert!(style.palette(&types).unwrap_err().contains("lava"));
    }

    /// A small map with every type in row 0, units and a 2x1 structure in
    /// row 1: each texel is the type colour or the occupancy colour, with the
    /// right state.
    #[test]
    fn cell_colour_follows_type_and_occupancy() {
        let (types, palette) = palette();
        let ids: Vec<u8> = types.iter().map(|t| t.id).collect();
        let width = ids.len() as u32;
        let grass = types.id_of("grass").unwrap();
        let water = types.id_of("water").unwrap();
        let rock = types.id_of("rock").unwrap();
        let mut cells = ids.clone();
        // Row 1: grass, grass (structure), grass (structure), water (units), grass...
        let mut row1 = vec![grass; ids.len()];
        row1[3] = water;
        cells.extend(row1);
        // The sim's rule: water and rock are not open ground.
        let impassable = |id: u8| id == water || id == rock;
        let mut layer = CellLayer::new(width, 2, cells, impassable, palette.clone());

        for (x, &id) in ids.iter().enumerate() {
            let state = if impassable(id) {
                CellState::Impassable
            } else {
                CellState::Open
            };
            let [r, g, b] = palette.type_colour(id);
            assert_eq!(
                layer.texel(x as u32, 0),
                [r, g, b, state as u8],
                "type {id}"
            );
        }

        let rows = layer.update([(3, 1), (1, 1), (99, 99)], [(1, 1), (2, 1)]);
        assert_eq!(rows, Some(1..2), "only row 1 changed");
        let [r, g, b] = palette.structure;
        assert_eq!(layer.texel(1, 1), [r, g, b, CellState::Structure as u8]);
        assert_eq!(layer.texel(2, 1), [r, g, b, CellState::Structure as u8]);
        let [r, g, b] = palette.units;
        assert_eq!(layer.texel(3, 1), [r, g, b, CellState::Units as u8]);
        let [r, g, b] = palette.type_colour(grass);
        assert_eq!(layer.texel(0, 1), [r, g, b, CellState::Open as u8]);

        // Same occupancy again: nothing to upload. Units gone: one row back.
        assert_eq!(layer.update([(3, 1), (1, 1)], [(1, 1), (2, 1)]), None);
        assert_eq!(layer.update([], [(1, 1), (2, 1)]), Some(1..2));
        let [r, g, b] = palette.type_colour(water);
        assert_eq!(
            layer.texel(3, 1),
            [r, g, b, CellState::Impassable as u8],
            "water again"
        );
    }

    /// The upload writes the span of changed rows, not the whole map:
    /// rows 3..5 of a 10 x 8 layer are texels 30..50; rows past the map are
    /// cut off and an empty span uploads nothing.
    #[test]
    fn upload_covers_only_the_changed_span() {
        assert_eq!(upload_span((10, 8), 3..5), Some((3..5, 30..50)));
        assert_eq!(upload_span((10, 8), 7..12), Some((7..8, 70..80)));
        assert_eq!(upload_span((10, 8), 0..8), Some((0..8, 0..80)));
        assert_eq!(upload_span((10, 8), 4..4), None);
        assert_eq!(upload_span((10, 8), 9..12), None);
    }

    #[test]
    fn structure_wins_over_units_and_units_over_impassable() {
        assert_eq!(cell_state(true, true, true), CellState::Structure);
        assert_eq!(cell_state(true, true, false), CellState::Units);
        assert_eq!(cell_state(true, false, false), CellState::Impassable);
        assert_eq!(cell_state(false, false, false), CellState::Open);
    }
}
