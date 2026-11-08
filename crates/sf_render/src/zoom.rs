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

    /// The picture at `zoom`: models always at full weight, the icons as an overlay
    /// of [`ZoomBands::icon_alpha`].
    pub fn blend(&self, zoom: f32) -> Blend {
        Blend::overlay(self.icon_alpha(zoom))
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
        if !(self.model_scale_max.is_finite() && (1.0..=4.0).contains(&self.model_scale_max)) {
            return bad("zoom.model_scale_max must be between 1 and 4");
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
