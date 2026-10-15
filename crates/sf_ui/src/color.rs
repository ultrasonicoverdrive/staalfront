//! Colors and the RON-backed UI settings.

use serde::Deserialize;

/// An sRGB color with straight (non-premultiplied) alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

impl Color {
    pub const WHITE: Color = Color(255, 255, 255, 255);
    pub const TRANSPARENT: Color = Color(0, 0, 0, 0);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b, 255)
    }

    pub const fn to_array(self) -> [u8; 4] {
        [self.0, self.1, self.2, self.3]
    }

    /// Same color, alpha replaced.
    pub const fn with_alpha(self, a: u8) -> Self {
        Self(self.0, self.1, self.2, a)
    }
}

/// UI settings, loaded from `assets/config/ui.ron` (PLAN.md section 5 rule 6).
/// Every field has a default, so a RON file may list only what it changes.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// UI scale; the window's DPI scale factor is applied on top of it.
    pub scale: f32,
    /// Default text size in logical pixels.
    pub font_size: f32,
    pub panel: Color,
    pub button: Color,
    pub button_hover: Color,
    pub button_pressed: Color,
    pub button_disabled: Color,
    pub text: Color,
    pub text_disabled: Color,
    pub minimap_frame: Color,
    pub fov_frame: Color,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            scale: 1.0,
            font_size: 14.0,
            panel: Color(28, 34, 44, 235),
            button: Color(60, 96, 150, 255),
            button_hover: Color(84, 126, 188, 255),
            button_pressed: Color(38, 64, 108, 255),
            button_disabled: Color(70, 74, 82, 255),
            text: Color(236, 240, 246, 255),
            text_disabled: Color(130, 136, 146, 255),
            minimap_frame: Color(18, 22, 30, 255),
            fov_frame: Color(255, 255, 255, 235),
        }
    }
}

/// Smallest and largest effective UI scale (setting times DPI factor).
pub const MIN_SCALE: f32 = 0.5;
pub const MAX_SCALE: f32 = 4.0;

impl UiConfig {
    /// Parse a RON document (see `assets/config/ui.ron`).
    pub fn from_ron_str(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    /// The scale actually used: the setting times the window's DPI factor,
    /// kept in a sane range (a broken config must not make the UI unusable).
    pub fn effective_scale(&self, dpi: f64) -> f32 {
        let setting = if self.scale.is_finite() {
            self.scale
        } else {
            1.0
        };
        let dpi = if dpi.is_finite() && dpi > 0.0 {
            dpi as f32
        } else {
            1.0
        };
        (setting * dpi).clamp(MIN_SCALE, MAX_SCALE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_config_parses_and_equals_the_defaults() {
        let text = include_str!("../../../assets/config/ui.ron");
        assert_eq!(UiConfig::from_ron_str(text).unwrap(), UiConfig::default());
    }

    #[test]
    fn partial_config_keeps_defaults() {
        let c = UiConfig::from_ron_str("(scale: 2.0)").unwrap();
        assert_eq!(c.scale, 2.0);
        assert_eq!(c.button, UiConfig::default().button);
    }

    #[test]
    fn effective_scale_multiplies_dpi_and_clamps() {
        let c = UiConfig {
            scale: 1.5,
            ..UiConfig::default()
        };
        assert_eq!(c.effective_scale(2.0), 3.0);
        assert_eq!(c.effective_scale(10.0), MAX_SCALE);
        assert_eq!(c.effective_scale(0.0), 1.5);
        assert_eq!(c.effective_scale(f64::NAN), 1.5);
        let tiny = UiConfig {
            scale: 0.01,
            ..UiConfig::default()
        };
        assert_eq!(tiny.effective_scale(1.0), MIN_SCALE);
    }
}
