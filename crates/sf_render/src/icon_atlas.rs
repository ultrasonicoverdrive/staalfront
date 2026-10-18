//! The icon atlas (WP3.6): the pictures of the strategic icons as data, CPU side.
//!
//! Icons are 64 x 64 PNGs under `assets/icons`, **white on transparent**: the
//! alpha is the silhouette (a base shape), white inside it is where the
//! owner's colour shows, and the inner sign is black or cut out. The engine
//! tints the white and draws its own dark rim (`assets/shaders/icon.wgsl`);
//! `docs/rules/icons.md` is the guide for artists.
//!
//! # What is uploaded
//! Not the PNG itself but two signed distance fields made from it at load
//! (an exact Euclidean distance transform, [`sdf_layer`]): the silhouette
//! (`R`) and the sign (`G`), each in 8 bits, `0.5` at the edge, one step
//! being [`SDF_SPREAD`] texels over 128. The shader reads the distance to
//! the edge in screen pixels from them, so the edge, the rim, the outline of
//! a hovered or selected icon and the sign stay crisp at every size, which
//! plain 64 x 64 colour texels minified to 14 px would not. The layers are
//! one texture array, uploaded once ([`crate::Renderer::set_icon_atlas`]).
//!
//! # Names and layers
//! [`IconAtlas::load`] reads `icons.ron` (optional: icon name to file) and
//! takes every other `*.png` in the directory by its file stem. Layers 0 to 3
//! are the four plain base shapes ([`BASE_NAMES`], `base_square.png` and so
//! on; the copies embedded in the binary when a file is missing), so an
//! [`IconShape`] is an [`IconId`] without an atlas at hand and a missing icon
//! falls back to its base shape. Names that point at the same file share a
//! layer. A broken file never stops the game: it becomes a warning and the
//! name is missing (the base shape is drawn).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::icons::{IconId, IconShape};

/// Side of an icon image in texels.
pub const ICON_SIDE: usize = 64;
/// Texels the distance fields reach on each side of the edge.
pub const SDF_SPREAD: f32 = 16.0;
/// The most layers an atlas holds (the usual limit of a texture array).
pub const MAX_LAYERS: usize = 256;
/// Bytes of one layer: two 8-bit distance fields.
pub const LAYER_BYTES: usize = ICON_SIDE * ICON_SIDE * 2;
/// The name of the optional manifest next to the icons.
pub const MANIFEST: &str = "icons.ron";
/// Names of the plain base shapes, in the order of [`IconShape::ALL`].
pub const BASE_NAMES: [&str; 4] = [
    "base_square",
    "base_circle",
    "base_triangle",
    "base_halfcircle",
];

/// The plain shapes shipped inside the binary (the same files as under
/// `assets/icons`), so the renderer works without an assets directory.
const EMBEDDED_BASES: [&[u8]; 4] = [
    include_bytes!("../../../assets/icons/base_square.png"),
    include_bytes!("../../../assets/icons/base_circle.png"),
    include_bytes!("../../../assets/icons/base_triangle.png"),
    include_bytes!("../../../assets/icons/base_halfcircle.png"),
];

/// `icons.ron`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    icons: BTreeMap<String, String>,
}

/// Whether `name` is usable as an icon name: 1 to 48 of `a-z 0-9 _`.
pub fn valid_name(name: &str) -> bool {
    (1..=48).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The loaded pictures: names to layers, and the layers' distance fields.
#[derive(Clone, Debug)]
pub struct IconAtlas {
    names: BTreeMap<String, IconId>,
    layers: Vec<Box<[u8]>>,
    warnings: Vec<String>,
}

impl IconAtlas {
    /// The four plain base shapes only (the embedded files).
    pub fn base() -> Self {
        Self::from_entries(&BTreeMap::new(), |_| Err(not_found()), Vec::new())
    }

    /// Read the icons in `dir` (normally `assets/icons`): `icons.ron` if there
    /// is one, then every `*.png` not named in it. Never fails; what could
    /// not be read is in [`IconAtlas::warnings`] and falls back to the base
    /// shapes.
    pub fn load(dir: &Path) -> Self {
        let mut warnings = Vec::new();
        let mut entries: BTreeMap<String, String> = BTreeMap::new();
        match std::fs::read_to_string(dir.join(MANIFEST)) {
            Ok(text) => match ron::from_str::<Manifest>(&text) {
                Ok(m) => entries = m.icons,
                Err(e) => warnings.push(format!("{MANIFEST}: {e}; using the png files by name")),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => warnings.push(format!("{MANIFEST}: {e}")),
        }
        match std::fs::read_dir(dir) {
            Ok(read) => {
                let mut files: Vec<String> = read
                    .filter_map(Result::ok)
                    .filter_map(|f| f.file_name().into_string().ok())
                    .filter(|f| f.ends_with(".png"))
                    .collect();
                files.sort();
                for file in files {
                    let stem = file.trim_end_matches(".png").to_string();
                    entries.entry(stem).or_insert(file);
                }
            }
            Err(e) => warnings.push(format!(
                "icon directory {}: {e}; every icon is its plain shape",
                dir.display()
            )),
        }
        Self::from_entries(&entries, |file| std::fs::read(dir.join(file)), warnings)
    }

    /// The atlas of `entries` (icon name to file name), reading each file with
    /// `read`. [`IconAtlas::load`] with the file system taken out, for tests.
    pub fn from_entries(
        entries: &BTreeMap<String, String>,
        read: impl Fn(&str) -> std::io::Result<Vec<u8>>,
        mut warnings: Vec<String>,
    ) -> Self {
        let mut layers: Vec<Box<[u8]>> = Vec::new();
        let mut names: BTreeMap<String, IconId> = BTreeMap::new();
        // File name to the layer it became (None: it failed, said once).
        let mut by_file: BTreeMap<&str, Option<IconId>> = BTreeMap::new();

        for (i, base) in BASE_NAMES.iter().enumerate() {
            let id = IconId(i as u16);
            let own = entries.get(*base).and_then(|file| {
                let layer = load_layer(base, file, &read, &mut warnings);
                by_file.insert(file, layer.as_ref().map(|_| id));
                layer
            });
            layers.push(own.unwrap_or_else(|| embedded_layer(i)));
            names.insert((*base).to_string(), id);
        }
        for (name, file) in entries {
            if BASE_NAMES.contains(&name.as_str()) {
                continue;
            }
            if !valid_name(name) {
                warnings.push(format!(
                    "icon name {name:?}: use 1 to 48 of a-z 0-9 _; ignored"
                ));
                continue;
            }
            if file.contains(['/', '\\']) || file.contains("..") {
                warnings.push(format!("icon {name}: {file:?} is not a plain file name"));
                continue;
            }
            let id = match by_file.get(file.as_str()) {
                Some(found) => *found,
                None => {
                    let id = if layers.len() >= MAX_LAYERS {
                        warnings.push(format!(
                            "icon {name}: the atlas holds {MAX_LAYERS} pictures at most"
                        ));
                        None
                    } else {
                        load_layer(name, file, &read, &mut warnings).map(|layer| {
                            layers.push(layer);
                            IconId((layers.len() - 1) as u16)
                        })
                    };
                    by_file.insert(file, id);
                    id
                }
            };
            if let Some(id) = id {
                names.insert(name.clone(), id);
            }
        }
        Self {
            names,
            layers,
            warnings,
        }
    }

    /// The picture called `name`, if there is one.
    pub fn lookup(&self, name: &str) -> Option<IconId> {
        self.names.get(name).copied()
    }

    /// The picture a role is drawn with: the one called `name`, or the plain
    /// base shape `fallback` when it has none or the atlas does not know it.
    pub fn resolve(&self, name: Option<&str>, fallback: IconShape) -> IconId {
        name.and_then(|n| self.lookup(n))
            .unwrap_or_else(|| fallback.into())
    }

    /// Every name and its layer, ascending by name.
    pub fn names(&self) -> impl Iterator<Item = (&str, IconId)> {
        self.names.iter().map(|(n, &id)| (n.as_str(), id))
    }

    /// Number of layers (pictures), at least four.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Never true: the four base shapes are always there.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// The two distance fields of a layer ([`LAYER_BYTES`] bytes, `R G` per texel).
    pub fn layer(&self, id: IconId) -> Option<&[u8]> {
        self.layers.get(usize::from(id.0)).map(|l| &l[..])
    }

    /// What was wrong while loading (unreadable files, bad names), for the log.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

fn not_found() -> std::io::Error {
    std::io::ErrorKind::NotFound.into()
}

fn embedded_layer(base: usize) -> Box<[u8]> {
    let image = crate::image::decode_png(EMBEDDED_BASES[base]).expect("the embedded icon is a PNG");
    sdf_layer(&image.pixels).expect("the embedded icon is 64 x 64")
}

fn load_layer(
    name: &str,
    file: &str,
    read: &impl Fn(&str) -> std::io::Result<Vec<u8>>,
    warnings: &mut Vec<String>,
) -> Option<Box<[u8]>> {
    let bytes = match read(file) {
        Ok(b) => b,
        Err(e) => {
            warnings.push(format!("icon {name}: {file}: {e}"));
            return None;
        }
    };
    let image = match crate::image::decode_png(&bytes) {
        Ok(i) => i,
        Err(e) => {
            warnings.push(format!("icon {name}: {file}: {e}"));
            return None;
        }
    };
    if (image.width, image.height) != (ICON_SIDE as u32, ICON_SIDE as u32) {
        warnings.push(format!(
            "icon {name}: {file} is {}x{}, icons are {ICON_SIDE}x{ICON_SIDE}",
            image.width, image.height
        ));
        return None;
    }
    let layer = sdf_layer(&image.pixels)?;
    // A picture with no silhouette would be invisible.
    if layer.chunks_exact(2).all(|t| t[0] < 128) {
        warnings.push(format!("icon {name}: {file} is empty (no opaque pixel)"));
        return None;
    }
    Some(layer)
}

// ---- the distance fields -------------------------------------------------

/// The two 8-bit distance fields of an icon image (RGBA8, 64 x 64): `None`
/// if the pixel buffer is not that size.
///
/// * The **silhouette** is every texel with alpha of at least 128, plus every
///   transparent texel that cannot be reached from the border through
///   transparent texels (a sign cut out of the middle is inside the shape).
/// * The **sign** is the part of the silhouette that is black (average of
///   red, green and blue below 128) or transparent.
///
/// Each field is the signed distance to the edge of its mask in texels,
/// negative inside, stored as `0.5 - d / (2 * SDF_SPREAD)` in a byte.
pub fn sdf_layer(rgba: &[u8]) -> Option<Box<[u8]>> {
    let n = ICON_SIDE;
    if rgba.len() != n * n * 4 {
        return None;
    }
    let opaque: Vec<bool> = rgba.chunks_exact(4).map(|p| p[3] >= 128).collect();
    // Transparent texels connected to the border are outside.
    let mut outside = vec![false; n * n];
    let mut stack: Vec<usize> = Vec::new();
    for i in 0..n {
        for t in [i, (n - 1) * n + i, i * n, i * n + n - 1] {
            if !opaque[t] && !outside[t] {
                outside[t] = true;
                stack.push(t);
            }
        }
    }
    while let Some(t) = stack.pop() {
        let (x, y) = (t % n, t / n);
        for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx >= n as i32 || ny >= n as i32 {
                continue;
            }
            let u = ny as usize * n + nx as usize;
            if !opaque[u] && !outside[u] {
                outside[u] = true;
                stack.push(u);
            }
        }
    }
    let silhouette: Vec<bool> = outside.iter().map(|&o| !o).collect();
    let sign: Vec<bool> = (0..n * n)
        .map(|t| {
            let p = &rgba[t * 4..t * 4 + 4];
            let grey = (u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])) / 3;
            silhouette[t] && (!opaque[t] || grey < 128)
        })
        .collect();
    let (d_sil, d_sign) = (signed_distance(&silhouette), signed_distance(&sign));
    let mut out = Vec::with_capacity(LAYER_BYTES);
    for t in 0..n * n {
        out.push(encode_distance(d_sil[t]));
        out.push(encode_distance(d_sign[t]));
    }
    Some(out.into_boxed_slice())
}

/// A distance in texels as a byte.
pub fn encode_distance(d: f32) -> u8 {
    ((0.5 - d / (2.0 * SDF_SPREAD)).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A byte back to a distance in texels (negative inside).
pub fn decode_distance(byte: u8) -> f32 {
    (0.5 - f32::from(byte) / 255.0) * 2.0 * SDF_SPREAD
}

/// Signed distance from each texel centre to the edge of `inside` (half a
/// texel from the nearest texel of the other kind), negative inside. The
/// image border counts as outside. With nothing inside, everything is far.
fn signed_distance(inside: &[bool]) -> Vec<f32> {
    let n = ICON_SIDE;
    let m = n + 2;
    let mut padded = vec![false; m * m];
    for y in 0..n {
        for x in 0..n {
            padded[(y + 1) * m + x + 1] = inside[y * n + x];
        }
    }
    let to_outside = distance_to(&padded.iter().map(|&i| !i).collect::<Vec<_>>(), m);
    let to_inside = distance_to(&padded, m);
    let mut out = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let t = (y + 1) * m + x + 1;
            out.push(if padded[t] {
                -(to_outside[t] - 0.5)
            } else {
                to_inside[t] - 0.5
            });
        }
    }
    out
}

const FAR: f64 = 1.0e20;

/// Euclidean distance from every texel of the `m` x `m` grid to the nearest
/// texel of `features` (the exact transform of Felzenszwalb and Huttenlocher,
/// two passes of a one-dimensional lower envelope of parabolas). 1000 where
/// there is no feature at all.
fn distance_to(features: &[bool], m: usize) -> Vec<f32> {
    let mut grid: Vec<f64> = features
        .iter()
        .map(|&f| if f { 0.0 } else { FAR })
        .collect();
    let mut line = vec![0.0; m];
    let mut out = vec![0.0; m];
    let mut v = vec![0usize; m];
    let mut z = vec![0.0; m + 1];
    for x in 0..m {
        for y in 0..m {
            line[y] = grid[y * m + x];
        }
        transform_1d(&line, &mut out, &mut v, &mut z);
        for y in 0..m {
            grid[y * m + x] = out[y];
        }
    }
    for y in 0..m {
        line.copy_from_slice(&grid[y * m..(y + 1) * m]);
        transform_1d(&line, &mut out, &mut v, &mut z);
        grid[y * m..(y + 1) * m].copy_from_slice(&out);
    }
    grid.iter()
        .map(|&d2| {
            if d2 >= FAR / 2.0 {
                1000.0
            } else {
                d2.sqrt() as f32
            }
        })
        .collect()
}

/// One pass of the transform: `out[q] = min over p of (q - p)^2 + f[p]`.
fn transform_1d(f: &[f64], out: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    let mut k = 0;
    v[0] = 0;
    z[0] = -FAR;
    z[1] = FAR;
    for q in 1..n {
        let intersect = |p: usize| {
            ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q as f64 - p as f64))
        };
        let mut s = intersect(v[k]);
        while s <= z[k] {
            k -= 1;
            s = intersect(v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = FAR;
    }
    k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let d = q as f64 - v[k] as f64;
        *o = d * d + f[v[k]];
    }
}

#[cfg(test)]
mod tests;
