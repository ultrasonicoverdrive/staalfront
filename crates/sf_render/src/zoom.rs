//! The camera settings file and what the picture does with the zoom
//! (`assets/config/camera.ron`).
//!
//! Two parts:
//!
//! * [`CameraFile`]: the numbers of the game camera ([`CameraConfig`]) in degrees and
//!   metres, including `far_margin`, how much more than the map the farthest zoom
//!   shows.
//! * [`ZoomBands`]: what the units and buildings look like at a zoom. **Models are
//!   drawn at every zoom** (decision Tijmen, 08-11-2026: zooming out further must not
//!   turn the soldiers and buildings into symbols). The strategic icons are drawn
//!   *on top of* the models in the farthest band of the zoom range and fade in there
//!   ([`ZoomBands::icon_alpha`]); and the models grow a little toward the far end so
//!   they stay readable ([`ZoomBands::model_scale`], capped). Both are functions of the
//!   camera zoom (`0..=1`, logarithmic in the distance), so the band is the same
//!   fraction of the wheel travel on every map size and resolution.

use glam::DVec2;
use serde::Deserialize;
use std::fmt;

use crate::camera::CameraConfig;
use crate::icons::Blend;

/// The shipped camera settings, embedded at build time (like the terrain settings).
pub const EMBEDDED_CAMERA_CONFIG: &str = include_str!("../../../assets/config/camera.ron");

/// The `camera` section of `camera.ron`: [`CameraConfig`] with angles in degrees and
/// without the map size (that comes from the map).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraFile {
    pub margin: f64,
    pub min_distance: f64,
    pub far_margin: f64,
    pub pitch_near_deg: f64,
    pub pitch_far_deg: f64,
    pub fov_y_deg: f64,
    pub distance_per_notch: f64,
    pub zoom_smooth_secs: f64,
    pub pan_smooth_secs: f64,
    pub pan_speed: f64,
    pub start_zoom: f64,
    pub clearance: f64,
    pub height_smooth_secs: f64,
}

impl CameraFile {
    /// The camera configuration for a map of `map_size` metres.
    pub fn config(&self, map_size: DVec2) -> CameraConfig {
        CameraConfig {
            map_size,
            margin: self.margin,
            min_distance: self.min_distance,
            max_distance: None,
            far_margin: self.far_margin,
            pitch_near: self.pitch_near_deg.to_radians(),
            pitch_far: self.pitch_far_deg.to_radians(),
            fov_y: self.fov_y_deg.to_radians(),
            distance_per_notch: self.distance_per_notch,
            zoom_smooth_secs: self.zoom_smooth_secs,
            pan_smooth_secs: self.pan_smooth_secs,
            pan_speed: self.pan_speed,
            start_zoom: self.start_zoom,
            clearance: self.clearance,
            height_smooth_secs: self.height_smooth_secs,
        }
    }

    fn validate(&self) -> Result<(), CameraSettingsError> {
        let bad = |what: &'static str| Err(CameraSettingsError::Invalid(what));
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if !(self.margin.is_finite() && self.margin >= 0.0) {
            return bad("camera.margin must be 0 or more");
        }
        if !positive(self.min_distance) {
            return bad("camera.min_distance must be above 0");
        }
        if !(self.far_margin.is_finite() && (0.0..=4.0).contains(&self.far_margin)) {
            return bad("camera.far_margin must be between 0 and 4 (a share of the map size)");
        }
        if !(0.0..=90.0).contains(&self.pitch_near_deg)
            || !(0.0..=90.0).contains(&self.pitch_far_deg)
            || self.pitch_near_deg > self.pitch_far_deg
        {
            return bad(
                "camera pitches must be 0 to 90 degrees with pitch_near_deg <= pitch_far_deg",
            );
        }
        if !(self.fov_y_deg > 1.0 && self.fov_y_deg < 150.0) {
            return bad("camera.fov_y_deg must be between 1 and 150");
        }
        if !(self.distance_per_notch > 0.0 && self.distance_per_notch < 1.0) {
            return bad("camera.distance_per_notch must be between 0 and 1");
        }
        if !(positive(self.zoom_smooth_secs)
            && positive(self.pan_smooth_secs)
            && positive(self.height_smooth_secs)
            && positive(self.pan_speed))
        {
            return bad("camera smoothing times and pan_speed must be above 0");
        }
        if !(0.0..=1.0).contains(&self.start_zoom) {
            return bad("camera.start_zoom must be between 0 and 1");
        }
        if !(self.clearance.is_finite() && self.clearance >= 0.0) {
            return bad("camera.clearance must be 0 or more");
        }
        Ok(())
    }
}

/// The largest `zoom.model_scale_max` the file may set (decision 10-11-2026).
pub const MODEL_SCALE_CAP: f32 = 2.5;

/// The `zoom` section of `camera.ron`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoomBands {
    /// Zoom (`0..=1`) up to which the icons are fully transparent.
    pub icons_from: f32,
    /// Zoom from which the icons are fully opaque. Above `icons_from`.
    pub icons_full: f32,
    /// Zoom up to which the models keep their size.
    pub model_scale_from: f32,
    /// The scale of the models at the farthest zoom (a cap: never more).
    pub model_scale_max: f32,
    /// The most opaque the icons are ever drawn (`0 < cap <= 1`), so the model stays
    /// visible through them. Only the drawn opacity: the switches follow the ramp.
    pub icons_max_alpha: f32,
}

impl ZoomBands {
    /// The opacity of the strategic icons drawn over the models at `zoom`: 0 up to
    /// `icons_from`, 1 from `icons_full`, a smooth ramp in between.
    pub fn icon_alpha(&self, zoom: f32) -> f32 {
        smooth_ramp(zoom, self.icons_from, self.icons_full)
    }

    /// The factor the models are drawn larger by at `zoom`: 1 up to `model_scale_from`,
    /// `model_scale_max` at zoom 1, a smooth ramp in between. Never above the cap.
    pub fn model_scale(&self, zoom: f32) -> f32 {
        let t = smooth_ramp(zoom, self.model_scale_from, 1.0);
        (1.0 + (self.model_scale_max - 1.0) * t).clamp(1.0, self.model_scale_max.max(1.0))
    }

    /// The picture at `zoom` with the player's icon toggle (`ToggleIcons`, on by
    /// default): on = [`ZoomBands::blend`], off = models only at every zoom, so the
    /// click volume, ring and bars stay on the model.
    pub fn blend_toggled(&self, zoom: f32, icons_on: bool) -> Blend {
        if icons_on {
            self.blend(zoom)
        } else {
            Blend::MODELS
        }
    }

    /// The picture at `zoom`: models always at full weight, the icons as an overlay
    /// on the ramp [`ZoomBands::icon_alpha`], drawn at most `icons_max_alpha` opaque.
    pub fn blend(&self, zoom: f32) -> Blend {
        Blend::overlay_capped(self.icon_alpha(zoom), self.icons_max_alpha)
    }

    fn validate(&self) -> Result<(), CameraSettingsError> {
        let bad = |what: &'static str| Err(CameraSettingsError::Invalid(what));
        let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        if !(unit(self.icons_from) && unit(self.icons_full) && self.icons_from < self.icons_full) {
            return bad(
                "zoom.icons_from and zoom.icons_full must be in 0..=1 with icons_from < icons_full",
            );
        }
        if !unit(self.model_scale_from) || self.model_scale_from >= 1.0 {
            return bad("zoom.model_scale_from must be in 0..1");
        }
        if !(self.model_scale_max.is_finite()
            && (1.0..=MODEL_SCALE_CAP).contains(&self.model_scale_max))
        {
            return bad("zoom.model_scale_max must be between 1 and 2.5");
        }
        if !(self.icons_max_alpha.is_finite()
            && self.icons_max_alpha > 0.0
            && self.icons_max_alpha <= 1.0)
        {
            return bad("zoom.icons_max_alpha must be above 0 and at most 1");
        }
        Ok(())
    }
}

/// `0` at and below `from`, `1` at and above `to`, a smoothstep in between.
fn smooth_ramp(x: f32, from: f32, to: f32) -> f32 {
    if x.is_nan() || to <= from {
        return 0.0;
    }
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// All of `camera.ron`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraSettings {
    pub camera: CameraFile,
    pub zoom: ZoomBands,
}

/// Why a camera file was refused.
#[derive(Debug)]
pub enum CameraSettingsError {
    Parse(String),
    Invalid(&'static str),
}

impl fmt::Display for CameraSettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "cannot parse camera config: {e}"),
            Self::Invalid(e) => write!(f, "invalid camera config: {e}"),
        }
    }
}

impl std::error::Error for CameraSettingsError {}

impl CameraSettings {
    /// The settings embedded in the binary.
    pub fn embedded() -> Self {
        Self::from_ron(EMBEDDED_CAMERA_CONFIG).expect("embedded camera.ron is valid")
    }

    pub fn from_ron(text: &str) -> Result<Self, CameraSettingsError> {
        let settings: Self =
            ron::from_str(text).map_err(|e| CameraSettingsError::Parse(e.to_string()))?;
        settings.camera.validate()?;
        settings.zoom.validate()?;
        Ok(settings)
    }

    /// The camera configuration for a map of `map_size` metres.
    pub fn camera_config(&self, map_size: DVec2) -> CameraConfig {
        self.camera.config(map_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bands() -> ZoomBands {
        CameraSettings::embedded().zoom
    }

    /// The shipped numbers, pinned: a change here is a decision, not an accident.
    #[test]
    fn the_shipped_camera_file_is_pinned() {
        let s = CameraSettings::embedded();
        let c = s.camera;
        assert_eq!((c.margin, c.min_distance, c.far_margin), (16.0, 12.0, 1.0));
        assert_eq!(
            (c.pitch_near_deg, c.pitch_far_deg, c.fov_y_deg),
            (45.0, 80.0, 45.0)
        );
        assert_eq!(
            (c.distance_per_notch, c.zoom_smooth_secs, c.pan_smooth_secs),
            (0.85, 0.10, 0.12)
        );
        assert_eq!(
            (c.pan_speed, c.start_zoom, c.clearance, c.height_smooth_secs),
            (0.7, 0.30, 2.0, 0.25)
        );
        assert_eq!(
            s.zoom,
            ZoomBands {
                icons_from: 0.95,
                icons_full: 1.0,
                model_scale_from: 0.45,
                model_scale_max: 2.5,
                icons_max_alpha: 0.4
            }
        );
        // The old Rust defaults are the same numbers, now from the file.
        let cfg = CameraConfig::default();
        assert_eq!(cfg.map_size, DVec2::new(256.0, 256.0));
        assert_eq!(cfg.pitch_far, 80_f64.to_radians());
        assert_eq!(cfg.max_distance, None);
    }

    /// The icons are transparent up to `icons_from`, opaque from `icons_full`, and
    /// rise smoothly and monotonically in between.
    #[test]
    fn the_icon_overlay_fades_in_over_the_farthest_band() {
        let b = bands();
        assert_eq!(b.icon_alpha(0.0), 0.0);
        assert_eq!(b.icon_alpha(0.5), 0.0);
        assert_eq!(b.icon_alpha(b.icons_from), 0.0);
        assert_eq!(b.icon_alpha(1.0), 1.0);
        assert_eq!(b.icon_alpha(b.icons_full), 1.0);
        let mid = 0.5 * (b.icons_from + b.icons_full);
        assert!((b.icon_alpha(mid) - 0.5).abs() < 1e-6);
        // Just inside the band: a little, not nothing and not all.
        let early = b.icon_alpha(b.icons_from + 0.003);
        assert!(early > 0.0 && early < 0.05, "{early}");
        let mut previous = 0.0;
        for i in 0..=1000 {
            let a = b.icon_alpha(i as f32 / 1000.0);
            assert!((0.0..=1.0).contains(&a));
            assert!(a >= previous, "not monotone at {i}");
            // A smoothstep is at most 1.5 times steeper than a line over the band.
            let steepest = 1.5 * 0.001 / (b.icons_full - b.icons_from);
            assert!(a - previous <= steepest + 1e-4, "a jump at {i}");
            previous = a;
        }
        // The far band is the last 5 % of the zoom range (decision 10-11-2026):
        // nothing at all at 0.90 or 0.95.
        assert!((1.0 - b.icons_from - 0.05).abs() < 1e-6);
        assert_eq!(b.icon_alpha(0.90), 0.0);
        assert_eq!(b.blend(0.90).icon(), 0.0);
        assert_eq!(b.blend(0.95).icon(), 0.0);
        assert_eq!(b.blend(0.90).dominant(), crate::icons::Dominant::Model);
        assert_eq!(b.icon_alpha(f32::NAN), 0.0);
    }

    /// Models are never hidden by the zoom alone: at every zoom the model weight is
    /// 1, whatever the icons do.
    #[test]
    fn the_zoom_alone_never_hides_a_model() {
        let b = bands();
        for i in 0..=1000 {
            let zoom = i as f32 / 1000.0;
            let blend = b.blend(zoom);
            assert_eq!(blend.model(), 1.0, "zoom {zoom}");
            assert!(blend.models_drawn());
            assert!((0..64).all(|k| blend.model_at(k % 8, k / 8)), "zoom {zoom}");
            assert_eq!(blend.ramp(), b.icon_alpha(zoom));
            assert!((blend.icon() - b.icon_alpha(zoom) * b.icons_max_alpha).abs() < 1e-6);
        }
        // And what the click volume is: the model until the icon is half there.
        assert_eq!(b.blend(0.0).dominant(), crate::icons::Dominant::Model);
        assert_eq!(b.blend(1.0).dominant(), crate::icons::Dominant::Icon);
    }

    /// At the farthest zoom the icon is drawn at the cap (40 %), so the model shows
    /// through; the switch from model to icon sits in the middle of the band whatever
    /// the cap is.
    #[test]
    fn the_icons_are_capped_and_the_switch_ignores_the_cap() {
        let b = bands();
        assert_eq!(b.icons_max_alpha, 0.4);
        let far = b.blend(1.0);
        assert!((far.icon() - 0.4).abs() < 1e-6, "{}", far.icon());
        assert_eq!(far.ramp(), 1.0);
        let low = ZoomBands {
            icons_max_alpha: 0.2,
            ..b
        };
        let switch = |bands: &ZoomBands| {
            (0..=1000)
                .map(|i| i as f32 / 1000.0)
                .find(|&z| bands.blend(z).dominant() == crate::icons::Dominant::Icon)
        };
        let mid = 0.5 * (b.icons_from + b.icons_full);
        let at = switch(&b).expect("the icon takes over");
        assert!((at - mid).abs() < 0.002, "{at}");
        assert_eq!(
            switch(&low),
            Some(at),
            "a lower cap does not move the switch"
        );
    }

    /// The model scale is 1 up to its start, the cap at the farthest zoom, never
    /// above the cap, and smooth.
    #[test]
    fn the_model_scale_is_capped_and_smooth() {
        let b = bands();
        assert_eq!(b.model_scale(0.0), 1.0);
        assert_eq!(b.model_scale(b.model_scale_from), 1.0);
        assert_eq!(b.model_scale(1.0), b.model_scale_max);
        assert_eq!(
            b.model_scale(5.0),
            b.model_scale_max,
            "past the end stays at the cap"
        );
        let mut previous = 1.0;
        for i in 0..=1000 {
            let s = b.model_scale(i as f32 / 1000.0);
            assert!((1.0..=b.model_scale_max).contains(&s), "{s}");
            assert!(s >= previous && s - previous < 0.02, "at {i}");
            previous = s;
        }
        assert!(b.model_scale(0.8) > 1.0 && b.model_scale(0.8) < b.model_scale_max);
    }

    /// The player's icon toggle: on is the band above, off is models only at every
    /// zoom, the model staying the click volume even at the farthest zoom.
    #[test]
    fn the_icon_toggle_off_draws_models_only() {
        let b = bands();
        assert_eq!(b.blend_toggled(1.0, true), b.blend(1.0));
        assert!(b.blend_toggled(1.0, true).icons_drawn());
        for i in 0..=100 {
            let zoom = i as f32 / 100.0;
            let off = b.blend_toggled(zoom, false);
            assert!(!off.icons_drawn(), "zoom {zoom}");
            assert_eq!(off.model(), 1.0);
            assert_eq!(off.dominant(), crate::icons::Dominant::Model, "zoom {zoom}");
        }
    }

    /// The model scale never goes above 2.5 (the shipped value is the cap).
    #[test]
    fn the_model_scale_cap_is_two_and_a_half() {
        let b = bands();
        assert_eq!(MODEL_SCALE_CAP, 2.5);
        assert_eq!(b.model_scale(1.0), 2.5);
        assert!((0..=100).all(|i| b.model_scale(i as f32 / 100.0) <= 2.5));
    }

    #[test]
    fn bad_camera_files_are_refused() {
        let base = EMBEDDED_CAMERA_CONFIG;
        assert!(CameraSettings::from_ron(base).is_ok());
        for (from, to) in [
            ("far_margin: 1.0", "far_margin: -1.0"),
            ("far_margin: 1.0", "far_margin: 9.0"),
            ("icons_from: 0.95", "icons_from: 1.0"),
            ("icons_from: 0.95", "icons_from: 1.5"),
            ("model_scale_max: 2.5", "model_scale_max: 9.0"),
            ("model_scale_max: 2.5", "model_scale_max: 2.6"),
            ("model_scale_max: 2.5", "model_scale_max: 0.5"),
            ("model_scale_from: 0.45", "model_scale_from: 1.0"),
            ("icons_max_alpha: 0.4", "icons_max_alpha: 0.0"),
            ("icons_max_alpha: 0.4", "icons_max_alpha: 1.5"),
            ("pitch_near_deg: 45.0", "pitch_near_deg: 85.0"),
            ("min_distance: 12.0", "min_distance: 0.0"),
            ("far_margin: 1.0", "far_margin_typo: 1.0"),
        ] {
            let text = base.replacen(from, to, 1);
            assert_ne!(text, base, "{from}");
            assert!(CameraSettings::from_ron(&text).is_err(), "{to}");
        }
    }
}
