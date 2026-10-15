//! Engine map format (WP1.7).
//!
//! A map is a directory `assets/maps/<name>/` with three files:
//!
//! ```text
//! map.ron        header (below)
//! height.png     16-bit grayscale, (width+1) x (height+1) pixels: the height
//!                of every cell corner (vertex grid)
//! materials.png  8-bit grayscale, width x height pixels: the material index
//!                of every cell
//! ```
//!
//! ```text
//! (
//!     width: 64,                 // cells, 1 cell = 1 m
//!     height: 64,
//!     height_scale: 8.0,         // metres for a heightmap value of 65535
//!     materials: ["grass", "dirt", "rock"],   // index = position in the list
//!     heightmap: "height.png",
//!     material_map: "materials.png",
//! )
//! ```
//!
//! The format knows no game terrain types: a material is just a name the
//! renderer turns into a texture. Height is visual only (PLAN.md 1.3). The
//! world frame is `x` east, `z` south, cell `(cx, cz)` covers
//! `[cx, cx+1] x [cz, cz+1]` metres, vertex `(vx, vz)` sits at `(vx, vz)`.
//!
//! The loader ([`Map::load`], [`Map::parse`]) validates dimensions, file
//! sizes, bit depths and material indices and reports a [`MapError`]. This
//! crate has no GPU or simulation dependency, so the renderer, the editor and
//! headless tools can all read maps. Floating point is used only for the
//! visual height in metres and in [`generate`], which is not simulation code.

pub mod generate;

use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Smallest and largest accepted side length in cells. PLAN.md 1.3 plans maps
/// of 64 to 256; the limits are looser so tools and tests can use small maps.
pub const MIN_SIDE: u32 = 1;
pub const MAX_SIDE: u32 = 1024;
/// Most materials a map may list (one texture array layer each; 256 is the
/// default `max_texture_array_layers` of wgpu).
pub const MAX_MATERIALS: usize = 256;

/// The RON header `map.ron`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapHeader {
    pub width: u32,
    pub height: u32,
    /// Metres corresponding to a heightmap value of 65535 (0 = flat map).
    pub height_scale: f32,
    /// Material names; the material map holds indices into this list.
    pub materials: Vec<String>,
    /// File name of the heightmap, relative to the map directory.
    pub heightmap: String,
    /// File name of the material index map, relative to the map directory.
    pub material_map: String,
}

/// Why a map could not be read.
#[derive(Debug)]
pub enum MapError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Ron(String),
    Png {
        file: String,
        message: String,
    },
    /// A header value is out of range (size, scale, material list).
    Header(String),
    /// An image has the wrong size, bit depth or color type.
    Image {
        file: String,
        message: String,
    },
    /// A cell refers to a material that is not in the list.
    MaterialIndex {
        cell: (u32, u32),
        index: u8,
        materials: usize,
    },
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::Ron(e) => write!(f, "map.ron: {e}"),
            Self::Png { file, message } => write!(f, "{file}: not a valid PNG ({message})"),
            Self::Header(e) => write!(f, "map.ron: {e}"),
            Self::Image { file, message } => write!(f, "{file}: {message}"),
            Self::MaterialIndex {
                cell,
                index,
                materials,
            } => write!(
                f,
                "material map: cell ({}, {}) uses material {index} but the map lists only {materials}",
                cell.0, cell.1
            ),
        }
    }
}

impl std::error::Error for MapError {}

/// A loaded, validated map.
#[derive(Clone, Debug, PartialEq)]
pub struct Map {
    width: u32,
    height: u32,
    height_scale: f32,
    materials: Vec<String>,
    /// `(width+1) * (height+1)` vertex heights, row-major (`z` rows).
    heights: Vec<u16>,
    /// `width * height` material indices, row-major.
    cells: Vec<u8>,
}

impl Map {
    /// Build a map from raw parts, applying the same validation as the loader.
    pub fn from_parts(
        width: u32,
        height: u32,
        height_scale: f32,
        materials: Vec<String>,
        heights: Vec<u16>,
        cells: Vec<u8>,
    ) -> Result<Self, MapError> {
        validate_header(width, height, height_scale, &materials)?;
        let (vw, vh) = (width as usize + 1, height as usize + 1);
        if heights.len() != vw * vh {
            return Err(MapError::Image {
                file: "heights".into(),
                message: format!(
                    "expected {vw}x{vh} = {} height samples for a {width}x{height} map, got {}",
                    vw * vh,
                    heights.len()
                ),
            });
        }
        if cells.len() != width as usize * height as usize {
            return Err(MapError::Image {
                file: "cells".into(),
                message: format!(
                    "expected {width}x{height} = {} cells, got {}",
                    width as usize * height as usize,
                    cells.len()
                ),
            });
        }
        if let Some(i) = cells
            .iter()
            .position(|&c| usize::from(c) >= materials.len())
        {
            return Err(MapError::MaterialIndex {
                cell: (i as u32 % width, i as u32 / width),
                index: cells[i],
                materials: materials.len(),
            });
        }
        Ok(Self {
            width,
            height,
            height_scale,
            materials,
            heights,
            cells,
        })
    }

    /// Read `dir/map.ron` and the files it names.
    pub fn load(dir: &Path) -> Result<Self, MapError> {
        let read = |name: &str| {
            let path = dir.join(name);
            std::fs::read(&path).map_err(|source| MapError::Io { path, source })
        };
        let header = read("map.ron")?;
        let header_text = String::from_utf8_lossy(&header);
        let parsed: MapHeader =
            ron::from_str(&header_text).map_err(|e| MapError::Ron(e.to_string()))?;
        let heightmap = read(&parsed.heightmap)?;
        let material_map = read(&parsed.material_map)?;
        Self::parse(&header_text, &heightmap, &material_map)
    }

    /// Build a map from the header text and the two PNG files' bytes.
    pub fn parse(
        header_ron: &str,
        heightmap_png: &[u8],
        material_png: &[u8],
    ) -> Result<Self, MapError> {
        let header: MapHeader =
            ron::from_str(header_ron).map_err(|e| MapError::Ron(e.to_string()))?;
        validate_header(
            header.width,
            header.height,
            header.height_scale,
            &header.materials,
        )?;
        let heights = decode_gray(
            &header.heightmap,
            heightmap_png,
            png::BitDepth::Sixteen,
            (header.width + 1, header.height + 1),
        )?;
        let heights = heights
            .chunks_exact(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .collect();
        let cells = decode_gray(
            &header.material_map,
            material_png,
            png::BitDepth::Eight,
            (header.width, header.height),
        )?;
        Self::from_parts(
            header.width,
            header.height,
            header.height_scale,
            header.materials,
            heights,
            cells,
        )
    }

    /// Write the three files into `dir` (created if needed); the file names
    /// are `height.png` and `materials.png`.
    pub fn save(&self, dir: &Path) -> Result<(), MapError> {
        let io = |path: PathBuf| move |source| MapError::Io { path, source };
        std::fs::create_dir_all(dir).map_err(io(dir.to_path_buf()))?;
        let header = MapHeader {
            width: self.width,
            height: self.height,
            height_scale: self.height_scale,
            materials: self.materials.clone(),
            heightmap: "height.png".into(),
            material_map: "materials.png".into(),
        };
        let text = ron::ser::to_string_pretty(&header, ron::ser::PrettyConfig::default())
            .map_err(|e| MapError::Ron(e.to_string()))?;
        let path = dir.join("map.ron");
        std::fs::write(&path, text + "\n").map_err(io(path))?;
        let bytes: Vec<u8> = self.heights.iter().flat_map(|h| h.to_be_bytes()).collect();
        let path = dir.join("height.png");
        let png = encode_gray(
            "height.png",
            &bytes,
            png::BitDepth::Sixteen,
            (self.width + 1, self.height + 1),
        )?;
        std::fs::write(&path, png).map_err(io(path))?;
        let path = dir.join("materials.png");
        let png = encode_gray(
            "materials.png",
            &self.cells,
            png::BitDepth::Eight,
            (self.width, self.height),
        )?;
        std::fs::write(&path, png).map_err(io(path))?;
        Ok(())
    }

    /// Map width in cells (= metres).
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Map height (north-south extent) in cells (= metres).
    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn height_scale(&self) -> f32 {
        self.height_scale
    }

    pub fn materials(&self) -> &[String] {
        &self.materials
    }

    /// Raw vertex heights, `(width+1) * (height+1)`, row-major.
    pub fn heights(&self) -> &[u16] {
        &self.heights
    }

    /// Raw material indices, `width * height`, row-major.
    pub fn cells(&self) -> &[u8] {
        &self.cells
    }

    /// Raw height sample of vertex `(vx, vz)`; coordinates are clamped to the grid.
    pub fn raw_height(&self, vx: i64, vz: i64) -> u16 {
        let x = vx.clamp(0, i64::from(self.width)) as usize;
        let z = vz.clamp(0, i64::from(self.height)) as usize;
        self.heights[z * (self.width as usize + 1) + x]
    }

    /// Height of vertex `(vx, vz)` in metres (coordinates clamped).
    pub fn vertex_height(&self, vx: i64, vz: i64) -> f32 {
        f32::from(self.raw_height(vx, vz)) / 65535.0 * self.height_scale
    }

    /// Material index of cell `(cx, cz)`; coordinates are clamped.
    pub fn material_at(&self, cx: i64, cz: i64) -> u8 {
        let x = cx.clamp(0, i64::from(self.width) - 1) as usize;
        let z = cz.clamp(0, i64::from(self.height) - 1) as usize;
        self.cells[z * self.width as usize + x]
    }

    /// Terrain height in metres at a world position (bilinear between the
    /// four corners of the cell; the position is clamped to the map).
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        let x = x.clamp(0.0, self.width as f32);
        let z = z.clamp(0.0, self.height as f32);
        let (x0, z0) = (x.floor(), z.floor());
        let (fx, fz) = (x - x0, z - z0);
        let (ix, iz) = (x0 as i64, z0 as i64);
        let h = |dx, dz| self.vertex_height(ix + dx, iz + dz);
        let north = h(0, 0) * (1.0 - fx) + h(1, 0) * fx;
        let south = h(0, 1) * (1.0 - fx) + h(1, 1) * fx;
        north * (1.0 - fz) + south * fz
    }
}

fn validate_header(
    width: u32,
    height: u32,
    scale: f32,
    materials: &[String],
) -> Result<(), MapError> {
    let side = MIN_SIDE..=MAX_SIDE;
    if !side.contains(&width) || !side.contains(&height) {
        return Err(MapError::Header(format!(
            "width and height must be {MIN_SIDE}..={MAX_SIDE} cells, got {width}x{height}"
        )));
    }
    if !scale.is_finite() || scale < 0.0 {
        return Err(MapError::Header(format!(
            "height_scale must be a finite number >= 0, got {scale}"
        )));
    }
    if materials.is_empty() || materials.len() > MAX_MATERIALS {
        return Err(MapError::Header(format!(
            "the material list must have 1..={MAX_MATERIALS} entries, got {}",
            materials.len()
        )));
    }
    for (i, name) in materials.iter().enumerate() {
        if name.trim().is_empty() {
            return Err(MapError::Header(format!("material {i} has an empty name")));
        }
        if let Some(j) = materials[..i].iter().position(|m| m == name) {
            return Err(MapError::Header(format!(
                "material name \"{name}\" appears twice (entries {j} and {i})"
            )));
        }
    }
    Ok(())
}

/// Decode a grayscale PNG of the expected depth and size to raw bytes
/// (16-bit samples stay big-endian as stored in the file).
fn decode_gray(
    file: &str,
    bytes: &[u8],
    depth: png::BitDepth,
    (w, h): (u32, u32),
) -> Result<Vec<u8>, MapError> {
    let png_err = |e: png::DecodingError| MapError::Png {
        file: file.to_string(),
        message: e.to_string(),
    };
    let mut reader = png::Decoder::new(Cursor::new(bytes))
        .read_info()
        .map_err(png_err)?;
    let (color, bit_depth) = reader.output_color_type();
    if color != png::ColorType::Grayscale || bit_depth != depth {
        return Err(MapError::Image {
            file: file.to_string(),
            message: format!(
                "must be a {depth:?}-bit grayscale PNG, got {bit_depth:?}-bit {color:?}"
            ),
        });
    }
    let (iw, ih) = reader.info().size();
    if (iw, ih) != (w, h) {
        return Err(MapError::Image {
            file: file.to_string(),
            message: format!("must be {w}x{h} pixels for this map size, got {iw}x{ih}"),
        });
    }
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf).map_err(png_err)?;
    buf.truncate(info.buffer_size());
    Ok(buf)
}

fn encode_gray(
    file: &str,
    data: &[u8],
    depth: png::BitDepth,
    (w, h): (u32, u32),
) -> Result<Vec<u8>, MapError> {
    let png_err = |e: png::EncodingError| MapError::Png {
        file: file.to_string(),
        message: e.to_string(),
    };
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(depth);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().map_err(png_err)?;
    writer.write_image_data(data).map_err(png_err)?;
    writer.finish().map_err(png_err)?;
    Ok(out)
}

#[cfg(test)]
mod tests;
