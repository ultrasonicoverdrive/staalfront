//! Strategic icons (WP1.9, PLAN.md section 2.2): what the units look like when
//! the camera is far away, and how the picture changes from models to icons.
//!
//! CPU side only (no wgpu): the settings, the zoom-to-blend function, the
//! dither pattern and the GPU instance layout. The icon pass itself is in
//! [`crate::renderer`] and `assets/shaders/icon.wgsl`.
//!
//! # What an icon is
//! A flat picture (a base shape, square, circle, triangle or half-circle, with a
//! sign inside it) filled with the player's colour and outlined, drawn as one
//! screen-aligned quad of a **constant pixel size** around the unit's projected
//! position. The size is `size_px` in logical pixels times the UI scale (the
//! window's DPI factor times `ui.ron`'s `scale`), so it neither grows nor
//! shrinks with the zoom.
//!
//! The pictures are data (WP3.6): 64 x 64 PNGs under `assets/icons`, loaded into
//! an [`IconAtlas`](crate::icon_atlas::IconAtlas) (a texture array, uploaded
//! once) and chosen per unit by an [`IconId`], the layer. The four base
//! shapes are layers 0 to 3 ([`IconShape`]); a role without an icon of its own
//! (or whose icon is missing) gets its base shape. The atlas stores a signed
//! distance field made from each PNG, so the edge, the outline and the sign are
//! crisp at any scale, as the procedural shapes of WP1.9 were. All icons of a
//! frame are one instanced draw call.
//!
//! # When icons are drawn (zoom work package, 08-11-2026)
//! **Models are drawn at every zoom.** The strategic icons are an overlay: drawn
//! on top of the models in the farthest band of the zoom range, with an alpha
//! that rises from 0 to 1 over that band ([`crate::zoom::ZoomBands::icon_alpha`],
//! the numbers are in `assets/config/camera.ron`). Before this work package the
//! icons took the place of the models (a dithered cross-fade driven by the
//! apparent size of a reference unit); zooming out further turned the soldiers
//! and buildings into symbols, which is what the player did not want.
//!
//! The blend of a frame is a [`Blend`]: the model weight `m` (always 1 in play;
//! below 1 the models keep that share of their pixels by an 8x8 ordered dither,
//! a debug aid) and the icon alpha `a`. The icon's own anti-aliased edge and
//! this alpha are ordinary alpha blending on top of the scene.
//!
//! # Depth
//! Icons are **not depth tested**: they are drawn after the scene, over
//! terrain and models alike. They are a map symbol, and a symbol hidden by a
//! hill would be a unit the player cannot find. At the zoom where icons exist
//! the camera looks down at 70 degrees or more, where relief hides almost nothing; what a
//! player may know about enemy units is the business of the fog of war
//! (WP8.4), not of the depth buffer. The price: icons of units on both sides of
//! a ridge overlap on the screen without occluding anything. Picking follows the
//! drawing (see [`crate::pick`]).
//!
//! # Fog of war
//! Because icons are drawn last and without depth, a fog layer drawn into the
//! scene cannot hide them. The units the player cannot see are removed on the
//! CPU before [`crate::Instancer::build_frame`], from the models and the icons
//! alike.

use bytemuck::{Pod, Zeroable};
use serde::Deserialize;

/// The silhouette of a unit's icon: the plain base shape, which is also what
/// a role gets when its own icon is missing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum IconShape {
    /// Structures, and the placeholder cube.
    #[default]
    Square,
    /// Infantry, and the placeholder cylinder.
    Circle,
    /// Vehicles, and the placeholder cone.
    Triangle,
    /// Vessels: a bowl, flat deck up.
    HalfCircle,
}

impl IconShape {
    pub const ALL: [IconShape; 4] = [
        IconShape::Square,
        IconShape::Circle,
        IconShape::Triangle,
        IconShape::HalfCircle,
    ];

    /// The atlas layer of the plain shape (the first four layers).
    pub fn index(self) -> u32 {
        self as u32
    }
}

/// A picture in the icon atlas: its layer. The plain base shapes are layers 0
/// to 3, so an [`IconShape`] converts to an id without an atlas at hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IconId(pub u16);

impl IconId {
    /// The layer of the atlas texture array.
    pub fn layer(self) -> u32 {
        u32::from(self.0)
    }
}

impl From<IconShape> for IconId {
    fn from(shape: IconShape) -> Self {
        Self(shape.index() as u16)
    }
}

/// Side of the icon's art box as a multiple of the icon size: the 64 x 64
/// image is drawn this much larger than `size_px`, so the triangle (wider than
/// the click quad) and the square (the quad itself) both fit.
pub const ART_BOX: f32 = 1.2;

/// Icon settings (the `icons` section of `assets/config/units.ron`).
/// Sizes are logical pixels unless the name says metres.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IconConfig {
    /// Bounding diameter in metres of the reference unit whose apparent size
    /// at the focus the effects use for their zoom bands.
    pub reference_size: f32,
    /// Side of the icon quad, the same at every zoom.
    pub size_px: f32,
    /// Width of the dark outline inside the shape's edge.
    pub outline_px: f32,
    /// Width of the white outline outside the shape of a hovered icon.
    pub hover_out_px: f32,
    /// Width of the outline outside the shape of a selected icon (WP3.3).
    pub selected_out_px: f32,
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
            size_px: 14.0,
            outline_px: 1.5,
            hover_out_px: 1.0,
            selected_out_px: 2.5,
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
        if !(self.size_px.is_finite() && (4.0..=128.0).contains(&self.size_px)) {
            return Err("icons.size_px must be between 4 and 128");
        }
        if !(self.outline_px.is_finite() && (0.0..=self.size_px * 0.25).contains(&self.outline_px))
        {
            return Err("icons.outline_px must be between 0 and a quarter of icons.size_px");
        }
        if !(self.hover_out_px.is_finite() && (0.0..=self.size_px).contains(&self.hover_out_px)) {
            return Err("icons.hover_out_px must be between 0 and icons.size_px");
        }
        if !(self.selected_out_px.is_finite()
            && (0.0..=self.size_px).contains(&self.selected_out_px))
        {
            return Err("icons.selected_out_px must be between 0 and icons.size_px");
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

    /// Everything the icon pass needs in physical pixels at this UI scale:
    /// the sizes, the outline widths and the room the quad leaves around the
    /// art box. **Every length scales with the UI scale**, the outline widths
    /// and the margin included (the WP3.3 review found a fixed 4 px margin
    /// cutting off the outline of a selected triangle at scale 2).
    pub fn params(&self, ui_scale: f32) -> IconParams {
        let size_px = self.icon_px(ui_scale);
        let hover_px = self.hover_out_px * ui_scale;
        let selected_px = self.selected_out_px * ui_scale;
        IconParams {
            size_px,
            outline_px: self.outline_px * ui_scale,
            box_px: size_px * ART_BOX,
            hover_px,
            selected_px,
        }
    }
}

/// What the picture is made of: the models and the icons drawn over them.
///
/// Two independent numbers. The **model weight** `m`: the share of the models' pixels
/// that is kept (an 8x8 ordered dither, see [`Blend::model_at`]); 1 in play, below 1
/// only as a debug aid (`--unit-display icons` is 0). The **icon alpha** `a`: the
/// opacity of the icon overlay, 0 = no icon is drawn. Besides those, the **ramp**:
/// how far the zoom is into the icon band (`0..=1`), before the opacity cap of
/// `camera.ron` (`zoom.icons_max_alpha`). What switches with the zoom (the click
/// volume, the ring and bars, the effects) follows the ramp, so a cap never moves it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blend {
    model: f32,
    icon: f32,
    ramp: f32,
}

impl Blend {
    /// Models only (zoomed in; also the "no icons" setting).
    pub const MODELS: Blend = Blend {
        model: 1.0,
        icon: 0.0,
        ramp: 0.0,
    };
    /// Icons only (a debug setting: the models are not drawn).
    pub const ICONS: Blend = Blend {
        model: 0.0,
        icon: 1.0,
        ramp: 1.0,
    };

    /// Models at full weight with the icons over them at opacity `alpha`
    /// (clamped to `0..=1`; NaN = no icons). This is the picture in play.
    pub fn overlay(alpha: f32) -> Self {
        Self::overlay_capped(alpha, 1.0)
    }

    /// Models at full weight with the icons over them: `ramp` is the position in
    /// the icon band (clamped to `0..=1`, NaN = 0), the drawn opacity is
    /// `ramp * cap` (`cap` clamped to `0..=1`, NaN = 1). The switches follow `ramp`.
    pub fn overlay_capped(ramp: f32, cap: f32) -> Self {
        let ramp = if ramp.is_nan() {
            0.0
        } else {
            ramp.clamp(0.0, 1.0)
        };
        let cap = if cap.is_nan() {
            1.0
        } else {
            cap.clamp(0.0, 1.0)
        };
        Self {
            model: 1.0,
            icon: ramp * cap,
            ramp,
        }
    }

    /// A blend with this model weight (clamped to `0..=1`, NaN = 1) and the icon
    /// opacity `1 - model`: the models dissolve into the picture and the icons fade in
    /// as they go.
    pub fn from_model_weight(model: f32) -> Self {
        let model = if model.is_nan() {
            1.0
        } else {
            model.clamp(0.0, 1.0)
        };
        Self {
            model,
            icon: 1.0 - model,
            ramp: 1.0 - model,
        }
    }

    /// The share of the models' pixels that is kept.
    pub fn model(self) -> f32 {
        self.model
    }

    /// The opacity of the icons.
    pub fn icon(self) -> f32 {
        self.icon
    }

    /// The position in the icon band before the opacity cap (`0..=1`): what the
    /// click volume, the ring and bars and the effects switch on.
    pub fn ramp(self) -> f32 {
        self.ramp
    }

    pub fn models_drawn(self) -> bool {
        self.model > 0.0
    }

    pub fn icons_drawn(self) -> bool {
        self.icon > 0.0
    }

    /// The representation that is the click volume and carries the bars: the icon
    /// once the zoom is past the middle of the icon band (ramp above one half; the
    /// opacity cap does not move this), the model otherwise (also on a tie).
    pub fn dominant(self) -> Dominant {
        if self.icon > 0.0 && self.ramp > 0.5 {
            Dominant::Icon
        } else {
            Dominant::Model
        }
    }

    /// Whether the model is drawn at the screen pixel `(x, y)`: the dither
    /// threshold of the pixel is below the model weight. This is the test of
    /// `scene.wgsl`.
    pub fn model_at(self, x: u32, y: u32) -> bool {
        dither_threshold(x, y) < self.model
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

/// Room in physical pixels the icon quad leaves around its art box for an
/// outline that reaches `outline_reach` px outside the shape: the outline
/// rounded up and one pixel for the antialiased edge. (The pictures keep a
/// texel off the border of the art box, so the shape's own edge needs no more.)
/// A plain icon (reach 0) has one pixel, which also bounds its fragments:
/// `icon.wgsl` takes this per icon from its flags.
pub fn quad_margin(outline_reach: f32) -> f32 {
    outline_reach.max(0.0).ceil() + 1.0
}

/// Bit position of the atlas layer in [`IconInstance::flags`] (bits 16 to 31).
pub const ICON_LAYER_SHIFT: u32 = 16;

/// Bit in [`IconInstance::flags`]: draw the icon highlighted (the hover: a
/// thin white outline **outside** the shape, so the player colour keeps its
/// area).
pub const ICON_HIGHLIGHT: u32 = 1 << 8;

/// Bit in [`IconInstance::flags`]: the unit is selected (WP3.3). The icon
/// gets a white outline outside the shape, clearly thicker than the hover
/// highlight. With both bits set (hovering a selected icon) the outline keeps
/// the selected width and turns yellow, so none, hover, selected and both look
/// different, and the fill in the owner's colour is the same size in all of
/// them.
pub const ICON_SELECTED: u32 = 1 << 9;

/// Per-icon GPU data. The anchor is in world space; the vertex shader
/// projects it and offsets the quad's corners in pixels.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct IconInstance {
    /// World position of the icon's centre (the unit's ground point lifted by
    /// [`IconConfig::lift`]).
    pub position: [f32; 3],
    /// Bit 8: [`ICON_HIGHLIGHT`]. Bit 9: [`ICON_SELECTED`]. Bits 16 to 31:
    /// the atlas layer ([`IconId`]).
    pub flags: u32,
    /// Linear RGB fill, and the **opacity** of the icon in `a` (the overlay
    /// alpha of the frame).
    pub color: [f32; 4],
}

impl IconInstance {
    pub fn new(position: [f32; 3], icon: IconId, rgb: [f32; 3], blend: Blend) -> Self {
        Self {
            position,
            flags: icon.layer() << ICON_LAYER_SHIFT,
            color: [rgb[0], rgb[1], rgb[2], blend.icon()],
        }
    }

    /// The picture of the icon.
    pub fn icon(&self) -> IconId {
        IconId((self.flags >> ICON_LAYER_SHIFT) as u16)
    }

    pub fn highlighted(&self) -> bool {
        self.flags & ICON_HIGHLIGHT != 0
    }

    pub fn selected(&self) -> bool {
        self.flags & ICON_SELECTED != 0
    }
}

/// Per-frame icon uniforms: sizes in physical pixels ([`IconConfig::params`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconParams {
    /// Side of the icon's click quad (the nominal size).
    pub size_px: f32,
    /// Width of the dark rim inside the shape's edge.
    pub outline_px: f32,
    /// Side of the art box the 64 x 64 picture is drawn in: [`ART_BOX`] times
    /// `size_px`.
    pub box_px: f32,
    /// Width of the hover outline outside the shape.
    pub hover_px: f32,
    /// Width of the selection outline outside the shape.
    pub selected_px: f32,
}

impl IconParams {
    /// The uniform buffer, as `icon.wgsl`'s `IconParams` reads it after the
    /// viewport size.
    pub fn uniform(&self, viewport: [f32; 2]) -> [f32; 8] {
        [
            viewport[0],
            viewport[1],
            self.size_px,
            self.outline_px,
            self.box_px,
            self.hover_px,
            self.selected_px,
            0.0,
        ]
    }

    /// How far the outline of an icon with these flags reaches outside its
    /// shape: the selected width, else the hover width, else none (selected
    /// wins, as in the shader).
    pub fn reach(&self, flags: u32) -> f32 {
        if flags & ICON_SELECTED != 0 {
            self.selected_px
        } else if flags & ICON_HIGHLIGHT != 0 {
            self.hover_px
        } else {
            0.0
        }
    }

    /// Half the side of the drawn quad of an icon with these flags: the art
    /// box and the margin for its outline ([`quad_margin`]). The shader
    /// computes the same from the flags, so a plain icon is not drawn on the
    /// room that only a selected one needs.
    pub fn quad_half(&self, flags: u32) -> f32 {
        self.box_px * 0.5 + quad_margin(self.reach(flags))
    }
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
            IconConfig { size_px: 2.0, ..ok },
            IconConfig {
                outline_px: 9.0,
                ..ok
            },
            IconConfig { lift: -1.0, ..ok },
            IconConfig {
                hover_out_px: -0.5,
                ..ok
            },
            IconConfig {
                selected_out_px: 40.0,
                ..ok
            },
            IconConfig {
                selected_out_px: f32::NAN,
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
    fn the_model_shader_carries_the_dither_function() {
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
        // The icon shader has no dither any more: it blends by alpha.
        assert!(!include_str!("../../../assets/shaders/icon.wgsl").contains("dither_threshold"));
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

    /// The share of an 8x8 block's pixels the model keeps is its weight (to 1/64).
    #[test]
    fn model_pixels_follow_the_weight() {
        for step in 0..=128 {
            let m = step as f32 / 128.0;
            let blend = Blend::from_model_weight(m);
            let models = (0..64).filter(|i| blend.model_at(i % 8, i / 8)).count();
            assert!(
                (models as f32 / 64.0 - m).abs() <= 1.0 / 64.0 + 1e-6,
                "weight {m} keeps {models}/64 model pixels"
            );
        }
        assert!((0..64).all(|i| Blend::MODELS.model_at(i % 8, i / 8)));
        assert!((0..64).all(|i| !Blend::ICONS.model_at(i % 8, i / 8)));
        // A pixel exactly on its threshold is not the model's.
        for rank in 0..64 {
            let blend = Blend::from_model_weight((rank as f32 + 0.5) / 64.0);
            let (x, y) = (0..64)
                .map(|i| (i % 8, i / 8))
                .find(|&(x, y)| dither_rank(x, y) == rank)
                .unwrap();
            assert!(!blend.model_at(x, y), "rank {rank}: model on its threshold");
        }
    }

    /// The overlay keeps every model pixel and only sets the icon opacity.
    #[test]
    fn an_overlay_never_takes_a_model_pixel() {
        for step in 0..=100 {
            let a = step as f32 / 100.0;
            let b = Blend::overlay(a);
            assert_eq!(b.model(), 1.0);
            assert!((b.icon() - a).abs() < 1e-6);
            assert!(b.models_drawn());
            assert_eq!(b.icons_drawn(), a > 0.0);
            assert!((0..64).all(|i| b.model_at(i % 8, i / 8)), "alpha {a}");
        }
        assert_eq!(Blend::overlay(f32::NAN), Blend::MODELS);
        assert_eq!(Blend::overlay(3.0).icon(), 1.0);
        assert_eq!(Blend::overlay(-1.0), Blend::MODELS);
    }

    #[test]
    fn reference_size_shrinks_with_distance_and_inverts() {
        let c = IconConfig::default();
        let focal = 1300.0;
        let near = c.reference_px(focal, 50.0);
        let far = c.reference_px(focal, 100.0);
        assert!((near - 2.0 * far).abs() < 1e-3);
        let d = c.distance_for_px(focal, 20.0);
        assert!((c.reference_px(focal, d) - 20.0).abs() < 1e-3);
    }

    #[test]
    fn the_icon_is_dominant_above_half_opacity_and_the_model_on_a_tie() {
        assert_eq!(Blend::overlay(0.49).dominant(), Dominant::Model);
        assert_eq!(Blend::overlay(0.5).dominant(), Dominant::Model);
        assert_eq!(Blend::overlay(0.51).dominant(), Dominant::Icon);
        assert_eq!(Blend::from_model_weight(0.51).dominant(), Dominant::Model);
        assert_eq!(Blend::from_model_weight(0.49).dominant(), Dominant::Icon);
        assert_eq!(Blend::MODELS.dominant(), Dominant::Model);
        assert_eq!(Blend::ICONS.dominant(), Dominant::Icon);
        assert_eq!(Blend::from_model_weight(f32::NAN), Blend::MODELS);
        assert_eq!(Blend::from_model_weight(7.0), Blend::MODELS);
        assert_eq!(Blend::from_model_weight(-1.0), Blend::ICONS);
    }

    #[test]
    fn the_opacity_cap_dims_the_icon_but_never_moves_the_switch() {
        for cap in [1.0, 0.6, 0.4, 0.1] {
            for i in 0..=100 {
                let ramp = i as f32 / 100.0;
                let b = Blend::overlay_capped(ramp, cap);
                assert_eq!(b.ramp(), ramp);
                assert!((b.icon() - ramp * cap).abs() < 1e-6, "{ramp} {cap}");
                assert_eq!(b.model(), 1.0);
                assert_eq!(
                    b.dominant(),
                    Blend::overlay(ramp).dominant(),
                    "{ramp} {cap}"
                );
            }
        }
        let b = Blend::overlay_capped(1.0, 0.6);
        assert!((b.icon() - 0.6).abs() < 1e-6);
        assert_eq!(b.dominant(), Dominant::Icon);
        assert_eq!(Blend::overlay_capped(0.55, 0.4).dominant(), Dominant::Icon);
        assert_eq!(Blend::overlay_capped(f32::NAN, 0.6), Blend::MODELS);
    }

    #[test]
    fn icon_instance_layout_matches_the_shader() {
        assert_eq!(std::mem::size_of::<IconInstance>(), 32);
        let i = IconInstance::new(
            [1.0, 2.0, 3.0],
            IconShape::Triangle.into(),
            [0.1, 0.2, 0.3],
            Blend::from_model_weight(0.25),
        );
        assert_eq!(i.icon(), IconId(2));
        assert!(!i.highlighted());
        assert_eq!(i.color, [0.1, 0.2, 0.3, 0.75]);
        let h = IconInstance {
            flags: i.flags | ICON_HIGHLIGHT,
            ..i
        };
        assert!(h.highlighted());
        assert_eq!(h.icon(), IconId(2));
        // The layer, the highlight and the selection do not overlap, for any layer.
        let far = IconInstance::new([0.0; 3], IconId(u16::MAX), [0.0; 3], Blend::ICONS);
        assert_eq!(far.icon(), IconId(u16::MAX));
        assert_eq!(far.flags & (ICON_HIGHLIGHT | ICON_SELECTED), 0);
    }

    #[test]
    fn the_base_shapes_are_the_first_layers() {
        for (i, shape) in IconShape::ALL.iter().enumerate() {
            assert_eq!(IconId::from(*shape), IconId(i as u16));
            assert_eq!(shape.index() as usize, i);
        }
    }

    /// Every length of the icon pass scales with the UI scale, and the margin
    /// always has room for the widest outline (the WP3.3 review: a fixed 4 px
    /// margin cut off a selected triangle's outline at scale 2).
    #[test]
    fn the_params_scale_with_the_ui_scale_and_the_margin_holds_the_outline() {
        let c = IconConfig::default();
        let one = c.params(1.0);
        assert_eq!(one.size_px, 14.0);
        assert!((one.box_px - 14.0 * ART_BOX).abs() < 1e-4);
        for scale in [1.0_f32, 1.25, 1.5, 2.0, 3.0] {
            let p = c.params(scale);
            assert_eq!(p.size_px, (14.0 * scale).round());
            assert!((p.outline_px - c.outline_px * scale).abs() < 1e-5);
            assert!((p.hover_px - c.hover_out_px * scale).abs() < 1e-5);
            assert!((p.selected_px - c.selected_out_px * scale).abs() < 1e-5);
            assert!((p.box_px - p.size_px * ART_BOX).abs() < 1e-4);
            // The outline rounded up, then a pixel for the antialiased edge.
            for flags in [
                0,
                ICON_HIGHLIGHT,
                ICON_SELECTED,
                ICON_HIGHLIGHT | ICON_SELECTED,
            ] {
                let reach = p.reach(flags);
                assert!(p.quad_half(flags) >= p.box_px * 0.5 + reach.ceil() + 1.0);
                // The art box and the margin hold the click quad and the outline.
                assert!(p.quad_half(flags) >= p.size_px * 0.5 + reach + 1.0);
            }
            assert_eq!(p.reach(0), 0.0);
            assert_eq!(p.reach(ICON_HIGHLIGHT), p.hover_px);
            assert_eq!(p.reach(ICON_SELECTED | ICON_HIGHLIGHT), p.selected_px);
            // A plain icon's quad is the art box and one pixel, the smallest.
            assert_eq!(p.quad_half(0), p.box_px * 0.5 + 1.0);
            assert!(p.quad_half(0) < p.quad_half(ICON_SELECTED));
        }
        // At scale 2 the old fixed 4 px margin was smaller than the outline
        // (5 px) plus the triangle's overshoot; the new one grows with it.
        let (one, two) = (c.params(1.0), c.params(2.0));
        assert!(two.quad_half(ICON_SELECTED) - two.box_px * 0.5 > 4.0 + 1.0);
        assert!(
            two.quad_half(ICON_SELECTED) - two.box_px * 0.5
                > one.quad_half(ICON_SELECTED) - one.box_px * 0.5
        );
    }

    #[test]
    fn the_uniform_matches_the_shader_layout() {
        let u = IconConfig::default().params(2.0).uniform([640.0, 360.0]);
        assert_eq!(&u[..4], &[640.0, 360.0, 28.0, 3.0]);
        assert_eq!(u.len(), 8);
        // The shader declares the same eight floats in the same order.
        let shader = include_str!("../../../assets/shaders/icon.wgsl");
        let start = shader.find("struct IconParams").unwrap();
        let body = &shader[start..start + shader[start..].find("};").unwrap()];
        let names: Vec<&str> = body
            .lines()
            .filter_map(|l| {
                l.trim()
                    .strip_suffix(": f32,")
                    .or(l.trim().strip_suffix(": vec2<f32>,"))
            })
            .collect();
        assert_eq!(
            names,
            [
                "viewport",
                "size_px",
                "outline_px",
                "box_px",
                "hover_px",
                "selected_px"
            ]
        );
    }
}
