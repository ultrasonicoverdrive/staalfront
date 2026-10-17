//! The placeholder unit look as data (`assets/config/units.ron`): the player
//! color palette, the primitive shapes with their level-of-detail chains
//! (WP1.9) and the strategic icon settings (PLAN.md section 5 rule 6).

use std::fmt;

use serde::Deserialize;

use crate::icons::{IconConfig, IconShape};
use crate::mesh::{MIN_SEGMENTS, SEGMENTS};
use crate::model::MAX_LODS;
use crate::scene::MeshKind;

/// The shipped style, embedded at build time (like the shader).
pub const EMBEDDED_STYLE: &str = include_str!("../../../assets/config/units.ron");

#[derive(Deserialize)]
struct StyleFile {
    palette: Vec<PaletteEntry>,
    shapes: Vec<ShapeEntry>,
    #[serde(default)]
    lod: LodSettings,
    #[serde(default)]
    icons: IconConfig,
}

/// Model level-of-detail settings shared by all shapes.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LodSettings {
    /// A model goes to a coarser level only when its projected size is below
    /// the switch size times `1 - hysteresis`, and back to a finer one when it
    /// is above the switch size times `1 + hysteresis`. `0` = no hysteresis.
    pub hysteresis: f32,
}

impl Default for LodSettings {
    fn default() -> Self {
        Self { hysteresis: 0.15 }
    }
}

/// One level of a shape's chain, as written in the file.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LodSpec {
    /// Facets of a round shape (cylinder, cone) at this level; absent = the
    /// full-detail [`SEGMENTS`].
    #[serde(default)]
    pub segments: Option<u32>,
    /// The level is used while the model's projected bounding diameter is at
    /// least this many pixels. The last level of a chain says `0`.
    pub min_px: f32,
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
    /// The strategic icon; absent = a square.
    #[serde(default)]
    icon: IconShape,
    /// The level-of-detail chain, finest first; absent = one level.
    #[serde(default)]
    lods: Vec<LodSpec>,
}

/// One placeholder shape: which mesh and how big (metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub mesh: MeshKind,
    /// `(along the facing, height, across)`.
    pub size: [f32; 3],
    /// What the strategic icon looks like.
    pub icon: IconShape,
}

impl Shape {
    /// Diameter in metres of the sphere around the shape's box, whatever the
    /// facing: the size LOD selection and click volumes work with.
    pub fn bounding_diameter(&self) -> f32 {
        (self.size[0] * self.size[0] + self.size[1] * self.size[1] + self.size[2] * self.size[2])
            .sqrt()
    }
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
    lods: Vec<Vec<LodSpec>>,
    lod: LodSettings,
    icons: IconConfig,
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
            check_chain(s.mesh, &s.lods)?;
        }
        if !(0.0..1.0).contains(&file.lod.hysteresis) {
            return Err(StyleError::Invalid("lod.hysteresis must be in 0..1"));
        }
        file.icons.validate().map_err(StyleError::Invalid)?;
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
                    icon: s.icon,
                })
                .collect(),
            lods: file.shapes.iter().map(|s| s.lods.clone()).collect(),
            lod: file.lod,
            icons: file.icons,
        })
    }

    /// The same style with every shape `factor` times as big in each
    /// direction (levels of detail, palette and icons stay). The placeholder
    /// sizes in `units.ron` are about a metre; the demo scales them up to
    /// vehicle size for the 4 m cell (B12) without touching the file that the
    /// render tests are written against.
    pub fn scaled(&self, factor: f32) -> Style {
        let mut style = self.clone();
        for shape in &mut style.shapes {
            shape.size = shape.size.map(|v| v * factor);
        }
        style
    }

    /// The same palette, icon and level settings with other shapes (one per
    /// glTF model of a pack, WP1.10). The shapes get no placeholder level
    /// chains: their models bring their own.
    pub fn with_shapes(&self, shapes: Vec<Shape>) -> Result<Style, StyleError> {
        if shapes.is_empty() || shapes.len() > 255 {
            return Err(StyleError::Invalid("1 to 255 shapes"));
        }
        if shapes
            .iter()
            .any(|s| !s.size.iter().all(|v| v.is_finite() && *v > 0.0))
        {
            return Err(StyleError::Invalid("a shape size is not positive"));
        }
        Ok(Style {
            lods: vec![Vec::new(); shapes.len()],
            shapes,
            ..self.clone()
        })
    }

    /// This style with more shapes after its own (WP3.2): each variant is a
    /// shape of its own size and strategic icon that keeps the level-of-detail
    /// chain of an existing shape, `(chain_of, shape)` where `chain_of` is the
    /// index of the shape to copy it from. This is how one placeholder mesh
    /// (a cube) becomes a building of several footprints, or a cone of several
    /// sizes, without losing its chain of facets: [`ModelSet::from_style`]
    /// builds the variants' chains from the copies. The result may have no
    /// more than 255 shapes.
    ///
    /// [`ModelSet::from_style`]: crate::model::ModelSet::from_style
    pub fn with_variants(&self, variants: &[(u8, Shape)]) -> Result<Style, StyleError> {
        if self.shapes.len() + variants.len() > 255 {
            return Err(StyleError::Invalid("more than 255 shapes"));
        }
        let mut style = self.clone();
        for (chain_of, shape) in variants {
            if !shape.size.iter().all(|v| v.is_finite() && *v > 0.0) {
                return Err(StyleError::Invalid("a shape size is not positive"));
            }
            style.lods.push(self.lod_specs(*chain_of).to_vec());
            style.shapes.push(*shape);
        }
        Ok(style)
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

    /// The level-of-detail chain written for a shape (index wraps around as
    /// in [`Style::shape`]); empty = a single full-detail level.
    pub fn lod_specs(&self, index: u8) -> &[LodSpec] {
        &self.lods[usize::from(index) % self.lods.len()]
    }

    /// Hysteresis and other settings shared by all chains.
    pub fn lod_settings(&self) -> LodSettings {
        self.lod
    }

    /// Strategic icon and cross-fade settings.
    pub fn icons(&self) -> &IconConfig {
        &self.icons
    }
}

/// Whether a chain is usable for a shape of kind `mesh`: the levels'
/// switch sizes fall strictly, the last one says 0, and only round shapes
/// have facets to reduce.
fn check_chain(mesh: MeshKind, lods: &[LodSpec]) -> Result<(), StyleError> {
    if lods.len() > MAX_LODS {
        return Err(StyleError::Invalid("a shape has more than 4 levels"));
    }
    let round = matches!(mesh, MeshKind::Cylinder | MeshKind::Cone);
    for (i, level) in lods.iter().enumerate() {
        if let Some(n) = level.segments {
            if !round {
                return Err(StyleError::Invalid(
                    "segments only apply to a cylinder or a cone",
                ));
            }
            if !(MIN_SEGMENTS as u32..=128).contains(&n) {
                return Err(StyleError::Invalid("segments must be between 3 and 128"));
            }
        }
        let last = i + 1 == lods.len();
        if last {
            if level.min_px != 0.0 {
                return Err(StyleError::Invalid("the last level must have min_px 0"));
            }
        } else if !(level.min_px.is_finite() && level.min_px > 0.0) {
            return Err(StyleError::Invalid("a level's min_px must be positive"));
        }
        if i > 0 && level.min_px >= lods[i - 1].min_px {
            return Err(StyleError::Invalid(
                "the min_px of the levels must fall from the first to the last",
            ));
        }
    }
    Ok(())
}

/// The facet count of a round mesh when a level does not say.
pub const DEFAULT_SEGMENTS: u32 = SEGMENTS as u32;

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
    fn variants_follow_the_style_and_keep_the_chain_they_come_from() {
        let base = Style::embedded();
        let cone = base
            .shapes()
            .iter()
            .position(|s| s.mesh == MeshKind::Cone)
            .unwrap() as u8;
        let big = Shape {
            mesh: MeshKind::Cone,
            size: [6.0, 5.0, 6.0],
            icon: IconShape::Circle,
        };
        let style = base.with_variants(&[(cone, big), (0, big)]).unwrap();
        assert_eq!(style.shape_count(), base.shape_count() + 2);
        assert_eq!(&style.shapes()[..base.shape_count()], base.shapes());
        let n = base.shape_count() as u8;
        assert_eq!(style.shape(n), big);
        // The first took the cone's chain of facets, the second the cube's
        // (none: one level).
        assert_eq!(style.lod_specs(n), base.lod_specs(cone));
        assert!(!style.lod_specs(n).is_empty());
        assert_eq!(style.lod_specs(n + 1), base.lod_specs(0));
        // The rest of the style is the same.
        assert_eq!(style.color_count(), base.color_count());
        assert_eq!(style.lod_settings(), base.lod_settings());
        // No variants: the same style.
        assert_eq!(base.with_variants(&[]).unwrap(), base);
    }

    #[test]
    fn variants_are_checked() {
        let base = Style::embedded();
        let flat = Shape {
            mesh: MeshKind::Cube,
            size: [1.0, 0.0, 1.0],
            icon: IconShape::Square,
        };
        assert!(base.with_variants(&[(0, flat)]).is_err());
        let ok = Shape {
            size: [1.0; 3],
            ..flat
        };
        let many = vec![(0, ok); 253];
        assert!(base.with_variants(&many).is_err(), "3 + 253 is over 255");
        assert!(base.with_variants(&many[..252]).is_ok());
    }

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
    fn embedded_style_has_a_chain_for_the_round_shapes_and_valid_icons() {
        let s = Style::embedded();
        assert!(s.lod_specs(0).len() <= 1, "the cube keeps one level");
        for round in [1, 2] {
            let chain = s.lod_specs(round);
            assert!(chain.len() >= 3, "{chain:?}");
            assert_eq!(chain[0].segments.unwrap_or(DEFAULT_SEGMENTS), 16);
            let n: Vec<_> = chain.iter().filter_map(|l| l.segments).collect();
            assert!(n.windows(2).all(|w| w[0] > w[1]), "fewer facets: {n:?}");
        }
        s.icons().validate().unwrap();
        let shapes: Vec<_> = s.shapes().iter().map(|s| s.icon).collect();
        assert_eq!(
            shapes,
            [IconShape::Square, IconShape::Circle, IconShape::Triangle]
        );
        assert!((0.0..1.0).contains(&s.lod_settings().hysteresis));
    }

    #[test]
    fn a_style_without_lod_and_icon_sections_gets_the_defaults() {
        let s = Style::from_ron(
            "(palette: [(name: \"a\", srgb: (1, 2, 3))], shapes: [(mesh: Cube, size: (1.0, 1.0, 1.0))])",
        )
        .unwrap();
        assert_eq!(*s.icons(), IconConfig::default());
        assert_eq!(s.lod_settings(), LodSettings::default());
        assert!(s.lod_specs(0).is_empty());
        assert_eq!(s.shape(0).icon, IconShape::Square);
    }

    fn with_lods(mesh: &str, lods: &str) -> Result<Style, StyleError> {
        Style::from_ron(&format!(
            "(palette: [(name: \"a\", srgb: (1, 2, 3))],
              shapes: [(mesh: {mesh}, size: (1.0, 1.0, 1.0), lods: {lods})])"
        ))
    }

    #[test]
    fn lod_chains_are_validated() {
        let ok = "[(segments: Some(16), min_px: 40.0), (segments: Some(8), min_px: 12.0), (segments: Some(5), min_px: 0.0)]";
        assert!(with_lods("Cylinder", ok).is_ok());
        for bad in [
            // Not falling.
            "[(min_px: 10.0), (min_px: 20.0), (min_px: 0.0)]",
            "[(min_px: 20.0), (min_px: 20.0), (min_px: 0.0)]",
            // The last level must say 0, the others must not.
            "[(min_px: 20.0), (min_px: 5.0)]",
            "[(min_px: 0.0), (min_px: 0.0)]",
            // Too many levels.
            "[(min_px: 50.0), (min_px: 40.0), (min_px: 30.0), (min_px: 20.0), (min_px: 0.0)]",
            // Facets: out of range.
            "[(segments: Some(2), min_px: 10.0), (min_px: 0.0)]",
            "[(segments: Some(500), min_px: 10.0), (min_px: 0.0)]",
        ] {
            assert!(with_lods("Cylinder", bad).is_err(), "{bad}");
        }
        // A cube has no facets to reduce.
        assert!(with_lods("Cube", "[(segments: Some(8), min_px: 0.0)]").is_err());
        assert!(with_lods("Cube", "[(min_px: 0.0)]").is_ok());
    }

    #[test]
    fn bad_lod_and_icon_settings_are_refused() {
        let base = |extra: &str| {
            Style::from_ron(&format!(
                "(palette: [(name: \"a\", srgb: (1, 2, 3))],
                  shapes: [(mesh: Cube, size: (1.0, 1.0, 1.0))], {extra})"
            ))
        };
        assert!(base("lod: (hysteresis: 0.2)").is_ok());
        assert!(base("lod: (hysteresis: 1.5)").is_err());
        assert!(base("lod: (hysteresis: -0.1)").is_err());
        assert!(base("icons: (size_px: 20.0)").is_ok());
        assert!(base("icons: (size_px: 1.0)").is_err());
        assert!(base("icons: (icon_below_px: 50.0)").is_err());
        assert!(base("icons: (nonsense: 1.0)").is_err());
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
