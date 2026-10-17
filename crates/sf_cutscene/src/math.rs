//! Small 2D maths for the scene format: time, vectors, colours, easing and
//! the 2x3 affine matrix that carries a part from its rig into the stage.
//!
//! Time is integer milliseconds ([`Ms`]) everywhere in the format. Geometry
//! (positions, angles, scales) is `f32`: this crate is presentation, not
//! simulation, so the determinism rules of `sf_sim` do not apply; what the tests
//! need to be reproducible is *when* things happen, and that is integer.

use std::fmt;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

/// Milliseconds. Signed so that a negative time in a script is an error we can
/// report ("negative time"), not a cryptic parse failure.
pub type Ms = i32;

/// A point or offset on the stage. `+x` is right, `+y` is **down** (screen
/// convention); the origin is the centre of the frame at camera zoom 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "(f32, f32)", into = "(f32, f32)")]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    pub const ONE: Vec2 = Vec2 { x: 1.0, y: 1.0 };

    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl From<(f32, f32)> for Vec2 {
    fn from((x, y): (f32, f32)) -> Vec2 {
        Vec2 { x, y }
    }
}

impl From<Vec2> for (f32, f32) {
    fn from(v: Vec2) -> (f32, f32) {
        (v.x, v.y)
    }
}

impl std::ops::Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f32) -> Vec2 {
        Vec2::new(self.x * k, self.y * k)
    }
}

/// An sRGB colour with alpha, written in RON as `"#rrggbb"` or `"#rrggbbaa"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// Parse `#rrggbb` or `#rrggbbaa`.
    pub fn parse(s: &str) -> Result<Color, String> {
        let hex = s
            .strip_prefix('#')
            .ok_or_else(|| format!("colour {s:?} must start with '#' (like \"#3a5f8a\")"))?;
        if !hex.is_ascii() || (hex.len() != 6 && hex.len() != 8) {
            return Err(format!(
                "colour {s:?} must be \"#rrggbb\" or \"#rrggbbaa\" (hex digits)"
            ));
        }
        let byte = |i: usize| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| format!("colour {s:?} has a non-hex digit"))
        };
        Ok(Color {
            r: byte(0)?,
            g: byte(2)?,
            b: byte(4)?,
            a: if hex.len() == 8 { byte(6)? } else { 255 },
        })
    }

    /// Multiply the colour channels (not alpha) by `num / 100`.
    pub fn scaled(self, percent: u32) -> Color {
        let k = |c: u8| (u32::from(c) * percent / 100).min(255) as u8;
        Color {
            r: k(self.r),
            g: k(self.g),
            b: k(self.b),
            a: self.a,
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.a == 255 {
            write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            write!(
                f,
                "#{:02x}{:02x}{:02x}{:02x}",
                self.r, self.g, self.b, self.a
            )
        }
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Color, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse(&s).map_err(de::Error::custom)
    }
}

/// How a value travels from its old value to its new one over an event's
/// duration. The input is progress `p` in `0..=1`, the output the blend weight
/// (`0` = old value, `1` = new value). Every curve maps `0 -> 0` and `1 -> 1`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Easing {
    /// Constant speed.
    Linear,
    /// Starts slowly (quadratic).
    EaseIn,
    /// Ends slowly (quadratic).
    EaseOut,
    /// Slow at both ends (quadratic in, quadratic out).
    #[default]
    EaseInOut,
    /// No in-between frames: the old value is held for the whole duration and
    /// the new one appears when it ends. (For an immediate change use a
    /// duration of 0, which any easing handles.)
    Step,
}

impl Easing {
    /// Blend weight for progress `p`; `p` is clamped to `0..=1`.
    pub fn apply(self, p: f32) -> f32 {
        let p = if p.is_nan() { 0.0 } else { p.clamp(0.0, 1.0) };
        match self {
            Easing::Linear => p,
            Easing::EaseIn => p * p,
            Easing::EaseOut => 1.0 - (1.0 - p) * (1.0 - p),
            Easing::EaseInOut => {
                if p < 0.5 {
                    2.0 * p * p
                } else {
                    1.0 - 2.0 * (1.0 - p) * (1.0 - p)
                }
            }
            Easing::Step => {
                if p >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

/// Values that can be blended; `w` is the eased weight (`0` = self, `1` = other).
pub trait Lerp: Clone {
    fn lerp(&self, other: &Self, w: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, other: &f32, w: f32) -> f32 {
        self + (other - self) * w
    }
}

impl Lerp for Vec2 {
    fn lerp(&self, other: &Vec2, w: f32) -> Vec2 {
        Vec2::new(self.x.lerp(&other.x, w), self.y.lerp(&other.y, w))
    }
}

/// A 2x3 affine matrix: `(x, y) -> (a*x + c*y + tx, b*x + d*y + ty)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    pub fn translation(v: Vec2) -> Affine {
        Affine {
            tx: v.x,
            ty: v.y,
            ..Affine::IDENTITY
        }
    }

    pub fn scaling(s: Vec2) -> Affine {
        Affine {
            a: s.x,
            d: s.y,
            ..Affine::IDENTITY
        }
    }

    /// Rotation by `degrees`. With `+y` down a positive angle turns clockwise
    /// on screen.
    pub fn rotation_deg(degrees: f32) -> Affine {
        let (s, c) = degrees.to_radians().sin_cos();
        Affine {
            a: c,
            b: s,
            c: -s,
            d: c,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// `self * rhs`: apply `rhs` first, then `self`.
    pub fn then_after(&self, rhs: &Affine) -> Affine {
        Affine {
            a: self.a * rhs.a + self.c * rhs.b,
            b: self.b * rhs.a + self.d * rhs.b,
            c: self.a * rhs.c + self.c * rhs.d,
            d: self.b * rhs.c + self.d * rhs.d,
            tx: self.a * rhs.tx + self.c * rhs.ty + self.tx,
            ty: self.b * rhs.tx + self.d * rhs.ty + self.ty,
        }
    }

    pub fn apply(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            self.a * p.x + self.c * p.y + self.tx,
            self.b * p.x + self.d * p.y + self.ty,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Easing; 5] = [
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::Step,
    ];

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn easing_endpoints_are_exact() {
        for e in ALL {
            assert_eq!(e.apply(0.0), 0.0, "{e:?} at 0");
            assert_eq!(e.apply(1.0), 1.0, "{e:?} at 1");
        }
    }

    #[test]
    fn easing_clamps_and_survives_nan() {
        for e in ALL {
            assert_eq!(e.apply(-3.0), 0.0);
            assert_eq!(e.apply(7.0), 1.0);
            assert_eq!(e.apply(f32::NAN), 0.0);
        }
    }

    #[test]
    fn easing_midpoints() {
        assert!(near(Easing::Linear.apply(0.5), 0.5));
        assert!(near(Easing::EaseIn.apply(0.5), 0.25));
        assert!(near(Easing::EaseOut.apply(0.5), 0.75));
        assert!(near(Easing::EaseInOut.apply(0.5), 0.5));
        assert!(near(Easing::EaseInOut.apply(0.25), 0.125));
        assert!(near(Easing::EaseInOut.apply(0.75), 0.875));
        // Step holds the old value until the very end.
        assert_eq!(Easing::Step.apply(0.5), 0.0);
        assert_eq!(Easing::Step.apply(0.999), 0.0);
    }

    #[test]
    fn easing_is_monotonic() {
        for e in ALL {
            let mut prev = 0.0;
            for i in 0..=100 {
                let w = e.apply(i as f32 / 100.0);
                assert!(w >= prev, "{e:?} decreases at {i}");
                prev = w;
            }
        }
    }

    #[test]
    fn colour_round_trip() {
        assert_eq!(
            Color::parse("#3a5f8a").unwrap(),
            Color::rgb(0x3a, 0x5f, 0x8a)
        );
        let c = Color::parse("#3a5f8a80").unwrap();
        assert_eq!(c.a, 0x80);
        assert_eq!(c.to_string(), "#3a5f8a80");
        assert_eq!(Color::rgb(1, 2, 3).to_string(), "#010203");
        assert!(Color::parse("3a5f8a").is_err());
        assert!(Color::parse("#3a5f8").is_err());
        assert!(Color::parse("#zzzzzz").is_err());
        assert!(Color::parse("#é12345").is_err());
    }

    #[test]
    fn affine_composition() {
        // Rotate a point 90 degrees clockwise on screen (+y down): right -> down.
        let r = Affine::rotation_deg(90.0);
        let p = r.apply(Vec2::new(1.0, 0.0));
        assert!(near(p.x, 0.0) && near(p.y, 1.0), "{p:?}");
        // Translate after rotate.
        let m = Affine::translation(Vec2::new(10.0, 20.0)).then_after(&r);
        let p = m.apply(Vec2::new(1.0, 0.0));
        assert!(near(p.x, 10.0) && near(p.y, 21.0), "{p:?}");
        // Scale then translate.
        let m = Affine::translation(Vec2::new(5.0, 0.0))
            .then_after(&Affine::scaling(Vec2::new(-1.0, 1.0)));
        let p = m.apply(Vec2::new(2.0, 3.0));
        assert!(near(p.x, 3.0) && near(p.y, 3.0), "{p:?}");
    }
}
