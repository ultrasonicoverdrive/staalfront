//! The placeholder unit look as data (`assets/config/units.ron`): the player
//! color palette and the primitive shapes (PLAN.md section 5 rule 6).

use std::fmt;

use serde::Deserialize;

use crate::scene::MeshKind;

/// The shipped style, embedded at build time (like the shader).
pub const EMBEDDED_STYLE: &str = include_str!("../../../assets/config/units.ron");

#[derive(Deserialize)]
struct StyleFile {
    palette: Vec<PaletteEntry>,
    shapes: Vec<ShapeEntry>,
}

#[derive(Deserialize)]
struct PaletteEntry {
    name: String,
    srgb: (u8, u8, u8),
}

#[derive(Deserialize)]
struct ShapeEntry {
    mesh: MeshKind,
    size: (f32, f32, f32),
}

/// One placeholder shape: which mesh and how big (metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub mesh: MeshKind,
    /// `(along the facing, height, across)`.
    pub size: [f32; 3],
}

/// Why a style file was refused.
#[derive(Debug)]
pub enum StyleError {
    Parse(String),
    Invalid(&'static str),
}

impl fmt::Display for StyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "cannot parse unit style: {e}"),
            Self::Invalid(e) => write!(f, "invalid unit style: {e}"),
        }
    }
}

impl std::error::Error for StyleError {}

/// Palette and shapes, ready for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    names: Vec<String>,
    srgb: Vec<[u8; 3]>,
    linear: Vec<[f32; 3]>,
    shapes: Vec<Shape>,
}

impl Style {
    /// The style embedded in the binary.
    pub fn embedded() -> Self {
        Self::from_ron(EMBEDDED_STYLE).expect("embedded units.ron is valid")
    }

    pub fn from_ron(text: &str) -> Result<Self, StyleError> {
        let file: StyleFile = ron::from_str(text).map_err(|e| StyleError::Parse(e.to_string()))?;
        if file.palette.is_empty() {
            return Err(StyleError::Invalid("the palette is empty"));
        }
        if file.shapes.is_empty() {
            return Err(StyleError::Invalid("there are no shapes"));
        }
        // The kind and player indices travel as u8 through the instancer.
        if file.palette.len() > 255 || file.shapes.len() > 255 {
            return Err(StyleError::Invalid("more than 255 colors or shapes"));
        }
        for s in &file.shapes {
            let (x, y, z) = s.size;
            if ![x, y, z].iter().all(|v| v.is_finite() && *v > 0.0) {
                return Err(StyleError::Invalid("a shape size is not positive"));
            }
            if s.mesh == MeshKind::Plane {
                return Err(StyleError::Invalid("a unit shape cannot be a plane"));
            }
        }
        Ok(Self {
            names: file.palette.iter().map(|p| p.name.clone()).collect(),
            srgb: file
                .palette
                .iter()
                .map(|p| [p.srgb.0, p.srgb.1, p.srgb.2])
                .collect(),
            linear: file
                .palette
                .iter()
                .map(|p| [p.srgb.0, p.srgb.1, p.srgb.2].map(srgb_to_linear))
                .collect(),
            shapes: file
                .shapes
                .iter()
                .map(|s| Shape {
                    mesh: s.mesh,
                    size: [s.size.0, s.size.1, s.size.2],
                })
                .collect(),
        })
    }

    pub fn color_count(&self) -> usize {
        self.linear.len()
    }

    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    pub fn color_name(&self, player: u8) -> &str {
        &self.names[usize::from(player) % self.names.len()]
    }

    /// The sRGB color of `player` (wraps around the palette).
    pub fn srgb(&self, player: u8) -> [u8; 3] {
        self.srgb[usize::from(player) % self.srgb.len()]
    }

    /// The linear-light color of `player` (wraps around the palette).
    pub fn linear(&self, player: u8) -> [f32; 3] {
        self.linear[usize::from(player) % self.linear.len()]
    }

    /// The shape with this index (wraps around).
    pub fn shape(&self, index: u8) -> Shape {
        self.shapes[usize::from(index) % self.shapes.len()]
    }

    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }
}

/// sRGB byte to linear light (the exact piecewise transfer function).
pub fn srgb_to_linear(byte: u8) -> f32 {
    let c = f32::from(byte) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_style_has_four_colors_and_three_shapes() {
        let s = Style::embedded();
        assert!(s.color_count() >= 4);
        let kinds: Vec<_> = s.shapes().iter().map(|s| s.mesh).collect();
        assert_eq!(kinds, [MeshKind::Cube, MeshKind::Cylinder, MeshKind::Cone]);
        // All colors are distinct.
        for a in 0..s.color_count() as u8 {
            for b in 0..a {
                assert_ne!(s.srgb(a), s.srgb(b));
            }
        }
    }

    #[test]
    fn indices_wrap_around() {
        let s = Style::embedded();
        assert_eq!(s.srgb(s.color_count() as u8), s.srgb(0));
        assert_eq!(s.shape(s.shape_count() as u8), s.shape(0));
    }

    #[test]
    fn srgb_conversion_hits_the_endpoints_and_midpoint() {
        assert_eq!(srgb_to_linear(0), 0.0);
        assert!((srgb_to_linear(255) - 1.0).abs() < 1e-6);
        // sRGB 128 is about 0.2159 linear.
        assert!((srgb_to_linear(128) - 0.2159).abs() < 1e-3);
    }

    #[test]
    fn bad_files_are_refused() {
        assert!(matches!(
            Style::from_ron("(palette: [], shapes: [(mesh: Cube, size: (1.0, 1.0, 1.0))])"),
            Err(StyleError::Invalid(_))
        ));
        assert!(matches!(
            Style::from_ron(
                "(palette: [(name: \"a\", srgb: (1, 2, 3))], shapes: [(mesh: Cube, size: (0.0, 1.0, 1.0))])"
            ),
            Err(StyleError::Invalid(_))
        ));
        assert!(matches!(
            Style::from_ron("nonsense"),
            Err(StyleError::Parse(_))
        ));
    }
}
