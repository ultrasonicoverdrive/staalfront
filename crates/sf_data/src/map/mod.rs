//! Engine map format (WP1.7).
//!
//! A map is a directory `assets/maps/<name>/` with three files, and an
//! optional fourth (WP2.1):
//!
//! ```text
//! map.ron        header (below)
//! height.png     16-bit grayscale, (width+1) x (height+1) pixels: the height
//!                of every cell corner (vertex grid)
//! materials.png  8-bit grayscale, width x height pixels: the material index
//!                of every cell
//! terrain.png    (optional) 8-bit grayscale, one pixel per 4 m gameplay cell:
//!                the terrain type id of every cell (see below)
//! ```
//!
//! ```text
//! (
//!     width: 64,                 // terrain cells, 1 terrain cell = 1 m
//!     height: 64,
//!     height_scale: 8.0,         // metres for a heightmap value of 65535
//!     materials: ["grass", "dirt", "rock"],   // index = position in the list
//!     heightmap: "height.png",
//!     material_map: "materials.png",
//!     terrain_map: Some("terrain.png"),   // optional; leave the line out for none
//! )
//! ```
//!
//! **Two kinds of cell** (PLAN.md decision B12). The map's `width` and `height`
//! count *terrain cells* of 1 m: the height and the material are stored per
//! metre, so every side of a gameplay cell (4 m, `sf_math::CELL_METRES`) has
//! four samples. A 1,024 x 1,024 map is a 256 x 256 cell playfield
//! ([`Map::size_in_cells`]). The gameplay data (terrain type, occupancy, fog)
//! is per gameplay cell.
//!
//! **Terrain types (WP2.1).** A material is just a name the renderer turns
//! into a texture. What a *gameplay* cell is (grass, road, water, rock, ore,
//! bridge) is the optional terrain layer: [`TerrainLayer`], stored as
//! `terrain.png`, an 8-bit grayscale PNG with one pixel per gameplay cell
//! whose value is the id of a type in the table [`TerrainTypes`]
//! (`assets/config/terrain_types.ron`). A PNG is what the other layers
//! already are, so the decoder, the checks and the tooling are shared; it is
//! tiny (a cell is a byte, and the layer has 16 times fewer pixels than the
//! material map, with long runs of equal values), and any image editor can
//! paint it, which suits the map editor of phase 10. A map whose header has no
//! `terrain_map` has no layer and loads as before. The loader refuses a layer
//! whose size is not [`Map::size_in_cells`] and a cell with an id the table
//! does not know. Height is visual only (PLAN.md 1.3). The
//! world frame is `x` east, `z` south, cell `(cx, cz)` covers
//! `[cx, cx+1] x [cz, cz+1]` metres, vertex `(vx, vz)` sits at `(vx, vz)`.
//!
//! The loader ([`Map::load`], [`Map::parse`]) validates dimensions, file
//! sizes, bit depths and material indices and reports a [`MapError`]. The two
//! file names in `map.ron` must stay inside the map directory: no absolute
//! path, drive prefix, network (UNC) path, `..` or Windows device name (`CON`),
//! checked the same on every OS (the optional `terrain_map` follows the same
//! rules); a symlink that leads out of the directory is refused when loading. This
//! crate has no GPU or simulation dependency, so the renderer, the editor and
//! headless tools can all read maps. Floating point is used only for the
//! visual height in metres and in [`generate`], which is not simulation code.

pub mod generate;

use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::terrain::{TerrainLayer, TerrainTypes};

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
    /// File name of the terrain layer, relative to the map directory; `None`
    /// for a map without terrain types (WP2.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain_map: Option<String>,
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
    /// A gameplay cell of the terrain layer has an id the table of terrain
    /// types does not define.
    TerrainId {
        cell: (u32, u32),
        id: u8,
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
            Self::TerrainId { cell, id } => write!(
                f,
                "terrain layer: cell ({}, {}) has terrain type id {id}, which the terrain types do not define",
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
    /// The terrain type of every gameplay cell, if the map has them.
    terrain: Option<TerrainLayer>,
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
            terrain: None,
        })
    }

    /// The same map with a terrain layer (replacing any earlier one).
    ///
    /// The layer must be exactly [`Map::size_in_cells`] cells and use only ids
    /// that `types` defines; the first cell (row-major) with an unknown id is
    /// reported.
    pub fn with_terrain(
        mut self,
        layer: TerrainLayer,
        types: &TerrainTypes,
    ) -> Result<Self, MapError> {
        let (cw, ch) = self.size_in_cells();
        if (layer.width(), layer.height()) != (cw, ch) {
            return Err(MapError::Image {
                file: "terrain".into(),
                message: format!(
                    "must be {cw}x{ch} cells for a {}x{} map, got {}x{}",
                    self.width,
                    self.height,
                    layer.width(),
                    layer.height()
                ),
            });
        }
        if let Some(i) = layer.ids().iter().position(|&id| types.get(id).is_none()) {
            return Err(MapError::TerrainId {
                cell: (i as u32 % cw, i as u32 / cw),
                id: layer.ids()[i],
            });
        }
        self.terrain = Some(layer);
        Ok(self)
    }

    /// Read `dir/map.ron` and the files it names.
    ///
    /// Every file is resolved first (symlinks followed) and must still lie
    /// inside the map directory, so a symlink in a downloaded map cannot lead
    /// out of it. (The check and the read are two steps; a map directory that
    /// another process changes while loading is not defended against.)
    ///
    /// A terrain layer is checked against the shipped table of terrain types
    /// ([`TerrainTypes::embedded`]); [`Map::load_with`] takes another table.
    pub fn load(dir: &Path) -> Result<Self, MapError> {
        Self::load_with(dir, &TerrainTypes::embedded())
    }

    /// [`Map::load`] with an explicit table of terrain types for the layer.
    pub fn load_with(dir: &Path, types: &TerrainTypes) -> Result<Self, MapError> {
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| MapError::Io { path, source }
        };
        let inside = dir.canonicalize().map_err(io(dir))?;
        let read = |name: &str| {
            let path = dir.join(name);
            let resolved = path.canonicalize().map_err(io(&path))?;
            if !resolved.starts_with(&inside) {
                return Err(MapError::Header(format!(
                    "{name} leads outside the map directory (a symlink?): it resolves to {}",
                    resolved.display()
                )));
            }
            std::fs::read(&resolved).map_err(io(&resolved))
        };
        let header = read("map.ron")?;
        let header_text = String::from_utf8_lossy(&header);
        let parsed: MapHeader =
            ron::from_str(&header_text).map_err(|e| MapError::Ron(e.to_string()))?;
        validate_file_names(&parsed)?;
        let heightmap = read(&parsed.heightmap)?;
        let material_map = read(&parsed.material_map)?;
        let terrain = parsed.terrain_map.as_deref().map(read).transpose()?;
        Self::parse_with(
            &header_text,
            &heightmap,
            &material_map,
            terrain.as_deref(),
            types,
        )
    }

    /// Build a map from the header text and the two PNG files' bytes. A header
    /// that names a terrain layer is refused here (it needs
    /// [`Map::parse_with`] and the layer's bytes).
    pub fn parse(
        header_ron: &str,
        heightmap_png: &[u8],
        material_png: &[u8],
    ) -> Result<Self, MapError> {
        Self::parse_with(
            header_ron,
            heightmap_png,
            material_png,
            None,
            &TerrainTypes::embedded(),
        )
    }

    /// Build a map from the header text and the bytes of its PNG files,
    /// including the terrain layer when the header names one (and only then:
    /// bytes without a `terrain_map` line, or a line without bytes, are an
    /// error). `types` is the table the layer's ids are checked against.
    pub fn parse_with(
        header_ron: &str,
        heightmap_png: &[u8],
        material_png: &[u8],
        terrain_png: Option<&[u8]>,
        types: &TerrainTypes,
    ) -> Result<Self, MapError> {
        let header: MapHeader =
            ron::from_str(header_ron).map_err(|e| MapError::Ron(e.to_string()))?;
        validate_header(
            header.width,
            header.height,
            header.height_scale,
            &header.materials,
        )?;
        validate_file_names(&header)?;
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
        let map = Self::from_parts(
            header.width,
            header.height,
            header.height_scale,
            header.materials,
            heights,
            cells,
        )?;
        match (&header.terrain_map, terrain_png) {
            (None, None) => Ok(map),
            (Some(name), Some(bytes)) => {
                let (cw, ch) = map.size_in_cells();
                let ids = decode_gray(name, bytes, png::BitDepth::Eight, (cw, ch))?;
                // The size was just checked, so the layer cannot fail.
                let layer = TerrainLayer::new(cw, ch, ids).map_err(|e| MapError::Image {
                    file: name.clone(),
                    message: e.to_string(),
                })?;
                map.with_terrain(layer, types)
            }
            (Some(name), None) => Err(MapError::Header(format!(
                "terrain_map \"{name}\" is named but its bytes were not given (use Map::parse_with)"
            ))),
            (None, Some(_)) => Err(MapError::Header(
                "a terrain layer was given but map.ron has no terrain_map".into(),
            )),
        }
    }

    /// Write the files into `dir` (created if needed); the file names are
    /// `height.png`, `materials.png` and, for a map with terrain types,
    /// `terrain.png`.
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
            terrain_map: self.terrain.as_ref().map(|_| TERRAIN_FILE.into()),
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
        if let Some(layer) = &self.terrain {
            let path = dir.join(TERRAIN_FILE);
            std::fs::write(&path, encode_terrain(layer)?).map_err(io(path))?;
        }
        Ok(())
    }

    /// Add (or replace) only the terrain layer of a map that is already in
    /// `dir`: writes `terrain.png` and sets `terrain_map` in `map.ron`,
    /// leaving the height and material files byte for byte as they are. Used
    /// to give a committed map a layer without regenerating its terrain.
    /// Does nothing if the map has no layer.
    pub fn save_terrain_layer(&self, dir: &Path) -> Result<(), MapError> {
        let Some(layer) = &self.terrain else {
            return Ok(());
        };
        let io = |path: PathBuf| move |source| MapError::Io { path, source };
        let header_path = dir.join("map.ron");
        let text = std::fs::read_to_string(&header_path).map_err(io(header_path.clone()))?;
        let mut header: MapHeader =
            ron::from_str(&text).map_err(|e| MapError::Ron(e.to_string()))?;
        header.terrain_map = Some(TERRAIN_FILE.into());
        let text = ron::ser::to_string_pretty(&header, ron::ser::PrettyConfig::default())
            .map_err(|e| MapError::Ron(e.to_string()))?;
        let path = dir.join(TERRAIN_FILE);
        std::fs::write(&path, encode_terrain(layer)?).map_err(io(path))?;
        std::fs::write(&header_path, text + "\n").map_err(io(header_path))?;
        Ok(())
    }

    /// The terrain type of every gameplay cell, if the map has them
    /// (WP2.1). The layer has [`Map::size_in_cells`] cells.
    pub fn terrain(&self) -> Option<&TerrainLayer> {
        self.terrain.as_ref()
    }

    /// Map width in terrain cells (= metres).
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Map height (north-south extent) in terrain cells (= metres).
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The map's size in gameplay cells of [`sf_math::CELL_METRES`] metres
    /// (rounded up when a side is not a whole number of cells): what the
    /// simulation's map size is.
    pub fn size_in_cells(&self) -> (u32, u32) {
        (
            self.width.div_ceil(sf_math::CELL_METRES),
            self.height.div_ceil(sf_math::CELL_METRES),
        )
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

/// The file name of the terrain layer [`Map::save`] writes.
const TERRAIN_FILE: &str = "terrain.png";

/// The file names of a header (two, and the terrain layer's when it has one)
/// must stay inside the map directory ([`check_file_name`]).
fn validate_file_names(header: &MapHeader) -> Result<(), MapError> {
    check_file_name("heightmap", &header.heightmap)?;
    check_file_name("material_map", &header.material_map)?;
    if let Some(name) = &header.terrain_map {
        check_file_name("terrain_map", name)?;
    }
    Ok(())
}

/// A terrain layer as an 8-bit grayscale PNG, one pixel per gameplay cell.
fn encode_terrain(layer: &TerrainLayer) -> Result<Vec<u8>, MapError> {
    encode_gray(
        TERRAIN_FILE,
        layer.ids(),
        png::BitDepth::Eight,
        (layer.width(), layer.height()),
    )
}

/// Refuse a file name from `map.ron` that could reach outside the map
/// directory (maps can come from other players: lobby, downloads).
///
/// The check is on the text, the same on every OS, because a map made on
/// Windows must be refused on Linux too: an absolute path (`/x`, `\x`), a
/// network path (`\\server\x`, `//server/x`), a drive prefix (`C:\x`, `C:x`,
/// and any `:` at all, which also keeps NTFS streams out), a `..` component
/// a backslash anywhere else (the separator of a map file name is `/`) and a
/// Windows device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`,
/// in any case, with or without an extension) in any component: opening one
/// does not read a file, and `CON` blocks. A relative name with `/` into a
/// sub-directory is fine. [`Map::load`] adds the symlink check on the real
/// file system.
pub fn check_file_name(field: &str, name: &str) -> Result<(), MapError> {
    let refuse = |why: &str| {
        Err(MapError::Header(format!(
            "{field} \"{name}\" must be a file name inside the map directory, but {why}"
        )))
    };
    if name.is_empty() {
        return refuse("it is empty");
    }
    if name.contains('\0') {
        return refuse("it contains a NUL character");
    }
    if name.starts_with("\\\\") || name.starts_with("//") {
        return refuse("it is a network (UNC) path");
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return refuse("it is an absolute path");
    }
    if name.contains(':') {
        return refuse("it has a drive prefix or a ':' (an absolute path on Windows)");
    }
    if name.split(['/', '\\']).any(|part| part == "..") {
        return refuse("it contains \"..\" (the parent directory)");
    }
    if name.contains('\\') {
        return refuse("it contains a backslash (use '/' between directories)");
    }
    if let Some(device) = name.split('/').find(|part| is_device_name(part)) {
        return refuse(&format!("\"{device}\" is a Windows device name"));
    }
    Ok(())
}

/// Whether a path component names a Windows device: the part before the first
/// dot, without trailing spaces (Windows ignores both), is `CON`, `PRN`,
/// `AUX`, `NUL`, `COM1`-`COM9` or `LPT1`-`LPT9`, in any case.
fn is_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or("").trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            upper
                .strip_prefix(prefix)
                .is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9']))
        })
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
                "must be a {}-bit grayscale PNG, got {}-bit {color:?}",
                bits(depth),
                bits(bit_depth)
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

/// The number of bits of a PNG bit depth ("8", not the `Debug` name "Eight").
fn bits(depth: png::BitDepth) -> u8 {
    match depth {
        png::BitDepth::One => 1,
        png::BitDepth::Two => 2,
        png::BitDepth::Four => 4,
        png::BitDepth::Eight => 8,
        png::BitDepth::Sixteen => 16,
    }
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

#[cfg(test)]
mod terrain_tests;
