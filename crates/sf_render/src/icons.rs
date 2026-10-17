//! Strategic icons (WP1.9, PLAN.md section 2.2): what the units look like when
//! the camera is far away, and how the picture changes from models to icons.
//!
//! CPU side only (no wgpu): the settings, the zoom-to-blend function, the
//! dither pattern and the GPU instance layout. The icon pass itself is in
//! [`crate::renderer`] and `assets/shaders/icon.wgsl`.
//!
//! # What an icon is
//! A flat shape (square, circle or triangle, one per unit shape) filled with
//! the player's colour and outlined, drawn as one screen-aligned quad of a
//! **constant pixel size** around the unit's projected position. The size is
//! `size_px` in logical pixels times the UI scale (the window's DPI factor
//! times `ui.ron`'s `scale`), so it neither grows nor shrinks with the zoom.
//! The shape is a signed distance function in the fragment shader (procedural:
//! crisp at any scale, no atlas). All icons of a frame are one instanced draw
//! call.
//!
//! # When icons take over
//! The trigger is the **apparent size of a reference unit at the camera
//! focus**: `reference_size` metres (the bounding diameter of a typical unit)
//! seen from the focus distance, in pixels. It is one number per frame, so
//! every unit crosses over at the same moment (no ring of icons moving over
//! the map) and the thresholds mean the same on every map size and on every
//! resolution (a camera "zoom" value would not: whole-map zoom is 340 m on a
//! 256 m map and 1360 m on a 1024 m map). The two thresholds are in logical
//! pixels and scale with the UI scale like the icon itself.
//!
//! * apparent size at or above `model_above_px`: models only;
//! * at or below `icon_below_px`: icons only (the models are not even built);
//! * in between: a cross-fade, linear in the logarithm of the apparent size.
//!   The zoom is logarithmic in the distance, so one wheel notch moves the
//!   blend by the same amount everywhere in the band.
//!
//! # The cross-fade is a dissolve
//! The blend is a single number, the **model weight** `m` in `0..=1`
//! (`icon weight = 1 - m`). Both representations read it as a threshold
//! against the same 8x8 ordered-dither pattern ([`dither_threshold`]): a
//! pixel belongs to the model when its threshold is below `m`, and to the
//! icon otherwise. So
//!
//! * the two weights sum to exactly 1 (one number, two complementary tests);
//! * a pixel is never both: nothing is drawn twice, so there is no double-bright overlap
//!   and no dark dip from half-transparent layers either;
//! * the models stay opaque and depth-correct (no sorting, no blending of
//!   overlapping units), only a share `m` of their pixels is kept;
//! * there is nothing to pop: at `m = 1` the dissolve test passes everywhere
//!   (identical to no dissolve), at `m = 0` the models are not drawn at all.
//!
//! The icon's own anti-aliased edge is ordinary alpha blending on top.
//!
//! # Depth
//! Icons are **not depth tested**: they are drawn after the scene, over
//! terrain and models alike. They are a map symbol, and a symbol hidden by a
//! hill would be a unit the player cannot find. At the zoom where icons exist
//! the camera looks down at 70 degrees or more, where relief hides almost nothing; what a
//! player may know about enemy units is the business of the fog of war
//! (WP8.4), not of the depth buffer. A depth-tested billboard would also be
//! half-eaten by the ground in front of it on a tilted camera, since the
//! ground below the icon on the screen is nearer to the eye than the icon's
//! anchor point. The price: icons of units on both sides of a ridge overlap
//! on the screen without occluding anything. Picking follows the drawing
//! (see [`crate::pick`]).

use bytemuck::{Pod, Zeroable};
use serde::Deserialize;

/// The silhouette of a unit's icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum IconShape {
    /// Buildings, and the placeholder cube.
    #[default]
    Square,
    /// The placeholder cylinder.
    Circle,
    /// Vehicles, and the placeholder cone.
    Triangle,
}

impl IconShape {
    pub const ALL: [IconShape; 3] = [IconShape::Square, IconShape::Circle, IconShape::Triangle];

    /// The number the shader switches on.
    pub fn index(self) -> u32 {
        self as u32
    }
}

/// Icon and cross-fade settings (the `icons` section of `assets/config/units.ron`).
/// Sizes are logical pixels unless the name says metres.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IconConfig {
    /// Bounding diameter in metres of the reference unit whose apparent size
    /// at the focus drives the cross-fade.
    pub reference_size: f32,
    /// Apparent size of the reference unit at or above which only models are
    /// drawn.
    pub model_above_px: f32,
    /// Apparent size at or below which only icons are drawn. Smaller than
    /// `model_above_px`; in between the two cross-fade.
    pub icon_below_px: f32,
    /// Side of the icon quad, the same at every zoom.
    pub size_px: f32,
    /// Width of the dark outline inside the shape's edge.
    pub outline_px: f32,
    /// How far above the ground the icon's anchor sits, in metres.
    pub lift: f32,
    /// Smallest size a model's click volume takes on the screen: a model
    /// smaller than this gets a bigger volume (the icon quad is the click
    /// volume once the icons are in charge, so this keeps the two alike).
    pub click_min_px: f32,
}

impl Default for IconConfig {
    fn default() -> Self {
        Self {
            reference_size: 2.0,
            model_above_px: 28.0,
            icon_below_px: 14.0,
            size_px: 14.0,
            outline_px: 1.5,
            lift: 0.5,
            click_min_px: 14.0,
        }
    }
}

/// Distances below this (metres) count as this, so a camera on top of the
/// focus cannot divide by zero.
const MIN_DISTANCE: f32 = 0.5;

impl IconConfig {
    /// Why these settings cannot be used, if they cannot.
    pub fn validate(&self) -> Result<(), &'static str> {
        let finite_positive = |v: f32| v.is_finite() && v > 0.0;
        if !finite_positive(self.reference_size) {
            return Err("icons.reference_size must be positive");
        }
        if !finite_positive(self.icon_below_px) || !finite_positive(self.model_above_px) {
            return Err("icons.icon_below_px and icons.model_above_px must be positive");
        }
        if self.icon_below_px >= self.model_above_px {
            return Err("icons.icon_below_px must be smaller than icons.model_above_px");
        }
        if !(self.size_px.is_finite() && (4.0..=128.0).contains(&self.size_px)) {
            return Err("icons.size_px must be between 4 and 128");
        }
        if !(self.outline_px.is_finite() && (0.0..=self.size_px * 0.25).contains(&self.outline_px))
        {
            return Err("icons.outline_px must be between 0 and a quarter of icons.size_px");
        }
        if !self.lift.is_finite() || self.lift < 0.0 {
            return Err("icons.lift must not be negative");
        }
        if !self.click_min_px.is_finite() || self.click_min_px < 0.0 {
            return Err("icons.click_min_px must not be negative");
        }
        Ok(())
    }

    /// Apparent size in physical pixels of the reference unit `distance`
    /// metres from the camera, for a camera with focal length `focal_px`
    /// (half the viewport height over `tan(fov_y / 2)`).
    pub fn reference_px(&self, focal_px: f32, distance: f32) -> f32 {
        self.reference_size * focal_px / distance.max(MIN_DISTANCE)
    }

    /// The camera distance at which the reference unit appears `px` pixels
    /// big (inverse of [`IconConfig::reference_px`]).
    pub fn distance_for_px(&self, focal_px: f32, px: f32) -> f32 {
        self.reference_size * focal_px / px.max(f32::MIN_POSITIVE)
    }

    /// Side of the icon quad in whole physical pixels (at least 1).
    pub fn icon_px(&self, ui_scale: f32) -> f32 {
        (self.size_px * ui_scale).round().max(1.0)
    }

    /// The model/icon weights when the reference unit appears `reference_px`
    /// physical pixels big, at UI scale `ui_scale`.
    pub fn blend(&self, reference_px: f32, ui_scale: f32) -> Blend {
        let hi = self.model_above_px * ui_scale;
        let lo = self.icon_below_px * ui_scale;
        if reference_px >= hi {
            return Blend::MODELS;
        }
        if reference_px <= lo {
            return Blend::ICONS;
        }
        // 0 at `hi`, 1 at `lo`, linear in the logarithm of the size.
        let t = (hi / reference_px).ln() / (hi / lo).ln();
        Blend::from_model_weight(1.0 - t)
    }

    /// [`IconConfig::blend`] from the camera: focal length and the distance
    /// from the eye to the focus.
    pub fn blend_for_camera(&self, focal_px: f32, distance: f32, ui_scale: f32) -> Blend {
        self.blend(self.reference_px(focal_px, distance), ui_scale)
    }
}

/// How much of the picture is models and how much is icons.
///
/// One number: the model weight `m`. The icon weight is `1 - m`, so the two
/// sum to exactly 1 by construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blend {
    model: f32,
}

impl Blend {
    /// Models only (zoomed in; also the "no icons" setting).
    pub const MODELS: Blend = Blend { model: 1.0 };
    /// Icons only (strategic zoom).
    pub const ICONS: Blend = Blend { model: 0.0 };

    /// A blend with this model weight, clamped to `0..=1`.
    pub fn from_model_weight(model: f32) -> Self {
        Self {
            model: if model.is_nan() {
                1.0
            } else {
                model.clamp(0.0, 1.0)
            },
        }
    }

    /// The share of the picture that is models.
    pub fn model(self) -> f32 {
        self.model
    }

    /// The share that is icons.
    pub fn icon(self) -> f32 {
        1.0 - self.model
    }

    pub fn models_drawn(self) -> bool {
        self.model > 0.0
    }

    pub fn icons_drawn(self) -> bool {
        self.model < 1.0
    }

    /// The representation that is more visible: the model on a tie.
    pub fn dominant(self) -> Dominant {
        if self.model >= 0.5 {
            Dominant::Model
        } else {
            Dominant::Icon
        }
    }

    /// Whether the model is drawn at the screen pixel `(x, y)`: the dither
    /// threshold of the pixel is below the model weight. This is the test of
    /// `scene.wgsl`.
    pub fn model_at(self, x: u32, y: u32) -> bool {
        dither_threshold(x, y) < self.model
    }

    /// Whether the icon is drawn at the screen pixel `(x, y)`: exactly where
    /// the model is not (the test of `icon.wgsl`).
    pub fn icon_at(self, x: u32, y: u32) -> bool {
        !self.model_at(x, y)
    }
}

/// Which of the two representations carries more of the picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dominant {
    Model,
    Icon,
}

/// Side of the dither pattern in pixels.
pub const DITHER_SIDE: u32 = 8;

/// Rank `0..64` of the pixel `(x, y)` in the 8x8 Bayer ordering: a
/// permutation of `0..64` over any aligned 8x8 block, ordered so that the
/// pixels with the lowest ranks are spread as evenly as possible. This
/// integer formula is also in `scene.wgsl` and `icon.wgsl`.
pub fn dither_rank(x: u32, y: u32) -> u32 {
    let x = x & 7;
    let y = y & 7;
    let xy = x ^ y;
    ((xy & 1) << 5)
        | ((y & 1) << 4)
        | ((xy & 2) << 2)
        | ((y & 2) << 1)
        | ((xy & 4) >> 1)
        | ((y & 4) >> 2)
}

/// The dither threshold of a pixel in `(0, 1)`: `(rank + 0.5) / 64`, so a
/// weight of exactly 0 keeps no pixel and exactly 1 keeps all of them.
pub fn dither_threshold(x: u32, y: u32) -> f32 {
    (dither_rank(x, y) as f32 + 0.5) / 64.0
}

/// One coordinate (physical pixels) of an icon's centre, moved so that the
/// edges of the quad fall on pixel boundaries: an even-sided quad is centred
/// on a pixel corner, an odd-sided one on a pixel centre. The vertex shader
/// does the same, and the picking uses it too, so the clickable quad is the
/// drawn one.
pub fn snap_icon_center(center: f32, size_px: f32) -> f32 {
    if (size_px.round() as i64) % 2 == 0 {
        (center + 0.5).floor()
    } else {
        center.floor() + 0.5
    }
}

/// Bit in [`IconInstance::flags`]: draw the icon highlighted (white outline).
pub const ICON_HIGHLIGHT: u32 = 1 << 8;

/// Per-icon GPU data. The anchor is in world space; the vertex shader
/// projects it and offsets the quad's corners in pixels.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct IconInstance {
    /// World position of the icon's centre (the unit's ground point lifted by
    /// [`IconConfig::lift`]).
    pub position: [f32; 3],
    /// Low byte: [`IconShape::index`]. Bit 8: [`ICON_HIGHLIGHT`].
    pub flags: u32,
    /// Linear RGB fill, and the **model weight** of the frame in `a`, the
    /// dissolve threshold (the icon is drawn where the model is not).
    pub color: [f32; 4],
}

impl IconInstance {
    pub fn new(position: [f32; 3], shape: IconShape, rgb: [f32; 3], blend: Blend) -> Self {
        Self {
            position,
            flags: shape.index(),
            color: [rgb[0], rgb[1], rgb[2], blend.model()],
        }
    }

    pub fn shape_index(&self) -> u32 {
        self.flags & 0xff
    }

    pub fn highlighted(&self) -> bool {
        self.flags & ICON_HIGHLIGHT != 0
    }
}

/// Per-frame icon uniforms: sizes in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconParams {
    /// Side of the icon quad.
    pub size_px: f32,
    /// Outline width.
    pub outline_px: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        IconConfig::default().validate().unwrap();
    }

    #[test]
    fn bad_configs_are_refused() {
        let ok = IconConfig::default();
        for bad in [
            IconConfig {
                reference_size: 0.0,
                ..ok
            },
            IconConfig {
                icon_below_px: 30.0,
                ..ok
            },
            IconConfig {
                icon_below_px: ok.model_above_px,
                ..ok
            },
            IconConfig {
                icon_below_px: -1.0,
                ..ok
            },
            IconConfig { size_px: 2.0, ..ok },
            IconConfig {
                outline_px: 9.0,
                ..ok
            },
            IconConfig { lift: -1.0, ..ok },
            IconConfig {
                model_above_px: f32::NAN,
                ..ok
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn dither_ranks_are_a_permutation_of_every_block() {
        for (bx, by) in [(0, 0), (8, 0), (0, 8), (24, 40), (1000, 1000)] {
            let mut seen = [false; 64];
            for y in 0..8 {
                for x in 0..8 {
                    let r = dither_rank(bx + x, by + y) as usize;
                    assert!(!seen[r], "rank {r} twice");
                    seen[r] = true;
                }
            }
            assert!(seen.iter().all(|&s| s));
        }
    }

    #[test]
    fn dither_spreads_the_first_pixels_evenly() {
        // Of any 2x2 sub-block, the ranks fall into four different quarters
        // of 0..64: a 25 % weight shows one pixel of every 2x2 block.
        for by in (0..8).step_by(2) {
            for bx in (0..8).step_by(2) {
                let mut quarters = [false; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    quarters[(dither_rank(bx + dx, by + dy) / 16) as usize] = true;
                }
                assert!(quarters.iter().all(|&q| q), "block {bx},{by}");
            }
        }
    }

    /// The model keeps the pixels the icon drops only if both shaders compute
    /// the same threshold. WGSL has no includes, so the function is written
    /// twice: it must be the same text in both, and the formula of
    /// [`dither_rank`].
    #[test]
    fn both_shaders_carry_the_same_dither_function() {
        let function = |src: &str| {
            let start = src
                .find("fn dither_threshold")
                .expect("the function exists");
            let end = start + src[start..].find("\n}").expect("it ends") + 2; // CRLF-safe
            src[start..end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        let scene = function(include_str!("../../../assets/shaders/scene.wgsl"));
        let icon = function(include_str!("../../../assets/shaders/icon.wgsl"));
        assert_eq!(scene, icon);
        assert!(scene.contains(
            "((xy & 1u) << 5u) | ((y & 1u) << 4u) | ((xy & 2u) << 2u) | ((y & 2u) << 1u) | ((xy & 4u) >> 1u) | ((y & 4u) >> 2u)"
        ));
        assert!(scene.contains("(f32(rank) + 0.5) / 64.0"));
    }

    #[test]
    fn dither_thresholds_stay_strictly_inside_the_unit_interval() {
        for y in 0..8 {
            for x in 0..8 {
                let t = dither_threshold(x, y);
                assert!(t > 0.0 && t < 1.0, "{t}");
            }
        }
    }

    /// The share of an 8x8 block's pixels the model keeps is its weight (to
    /// 1/64), and the icon keeps exactly the rest: every pixel belongs to
    /// exactly one of them, for every weight.
    #[test]
    fn model_and_icon_pixels_partition_every_block() {
        for step in 0..=128 {
            let m = step as f32 / 128.0;
            let blend = Blend::from_model_weight(m);
            let (mut models, mut icons) = (0, 0);
            for y in 0..8 {
                for x in 0..8 {
                    let (a, b) = (blend.model_at(x, y), blend.icon_at(x, y));
                    assert!(a != b, "pixel {x},{y} at weight {m} is drawn {a}/{b}");
                    models += u32::from(a);
                    icons += u32::from(b);
                }
            }
            assert_eq!(models + icons, 64);
            assert!(
                (models as f32 / 64.0 - m).abs() <= 1.0 / 64.0 + 1e-6,
                "weight {m} keeps {models}/64 model pixels"
            );
        }
        // The ends: no dissolve at all, and no model at all.
        assert!((0..64).all(|i| Blend::MODELS.model_at(i % 8, i / 8)));
        assert!((0..64).all(|i| Blend::ICONS.icon_at(i % 8, i / 8)));
    }

    #[test]
    fn weights_sum_to_one_across_the_band() {
        let c = IconConfig::default();
        for scale in [1.0, 1.5, 2.0] {
            let mut previous = 1.0_f32;
            for i in 0..=400 {
                // Sweep the apparent size from well above the band to well below.
                let px = c.model_above_px * scale * 1.5 * (0.2_f32 / 1.5).powf(i as f32 / 400.0);
                let b = c.blend(px, scale);
                assert!((b.model() + b.icon() - 1.0).abs() < 1e-6);
                assert!((0.0..=1.0).contains(&b.model()));
                // Zooming out (smaller apparent size) never brings models back.
                assert!(b.model() <= previous + 1e-6, "not monotone at {px}");
                previous = b.model();
            }
        }
    }

    #[test]
    fn the_band_has_the_documented_ends_and_is_linear_in_log_size() {
        let c = IconConfig::default();
        assert_eq!(c.blend(c.model_above_px, 1.0), Blend::MODELS);
        assert_eq!(c.blend(c.model_above_px * 3.0, 1.0), Blend::MODELS);
        assert_eq!(c.blend(c.icon_below_px, 1.0), Blend::ICONS);
        assert_eq!(c.blend(0.5, 1.0), Blend::ICONS);
        // The geometric mean of the ends is the middle of the band.
        let mid = (c.model_above_px * c.icon_below_px).sqrt();
        assert!((c.blend(mid, 1.0).model() - 0.5).abs() < 1e-5);
        // One third of the way (in log size) is one third of the weight.
        let third = c.model_above_px * (c.icon_below_px / c.model_above_px).powf(1.0 / 3.0);
        assert!((c.blend(third, 1.0).icon() - 1.0 / 3.0).abs() < 1e-5);
        // The thresholds scale with the UI scale.
        assert_eq!(
            c.blend(c.model_above_px * 2.0, 2.0),
            Blend::MODELS,
            "at scale 2 the band is twice as high"
        );
        assert!(c.blend(c.model_above_px * 1.5, 2.0).icon() > 0.0);
    }

    #[test]
    fn reference_size_shrinks_with_distance_and_inverts() {
        let c = IconConfig::default();
        let focal = 1300.0;
        let near = c.reference_px(focal, 50.0);
        let far = c.reference_px(focal, 100.0);
        assert!((near - 2.0 * far).abs() < 1e-3);
        let d = c.distance_for_px(focal, c.model_above_px);
        assert!((c.reference_px(focal, d) - c.model_above_px).abs() < 1e-3);
        assert_eq!(c.blend_for_camera(focal, d * 0.5, 1.0), Blend::MODELS);
        assert_eq!(c.blend_for_camera(focal, d * 4.0, 1.0), Blend::ICONS);
    }

    #[test]
    fn dominant_picks_the_larger_weight_and_the_model_on_a_tie() {
        assert_eq!(Blend::from_model_weight(0.51).dominant(), Dominant::Model);
        assert_eq!(Blend::from_model_weight(0.5).dominant(), Dominant::Model);
        assert_eq!(Blend::from_model_weight(0.49).dominant(), Dominant::Icon);
        assert_eq!(Blend::MODELS.dominant(), Dominant::Model);
        assert_eq!(Blend::ICONS.dominant(), Dominant::Icon);
        assert_eq!(Blend::from_model_weight(f32::NAN), Blend::MODELS);
        assert_eq!(Blend::from_model_weight(7.0), Blend::MODELS);
        assert_eq!(Blend::from_model_weight(-1.0), Blend::ICONS);
    }

    #[test]
    fn icon_instance_layout_matches_the_shader() {
        assert_eq!(std::mem::size_of::<IconInstance>(), 32);
        let i = IconInstance::new(
            [1.0, 2.0, 3.0],
            IconShape::Triangle,
            [0.1, 0.2, 0.3],
            Blend::from_model_weight(0.25),
        );
        assert_eq!(i.shape_index(), 2);
        assert!(!i.highlighted());
        assert_eq!(i.color, [0.1, 0.2, 0.3, 0.25]);
        let h = IconInstance {
            flags: i.flags | ICON_HIGHLIGHT,
            ..i
        };
        assert!(h.highlighted());
        assert_eq!(h.shape_index(), 2);
    }
}
