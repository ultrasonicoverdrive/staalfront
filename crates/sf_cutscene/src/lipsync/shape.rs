//! The nine mouth shapes, and what to draw when an artist has not drawn one.
//!
//! The set is the one Rhubarb Lip Sync documents: the basic six `A`-`F`, the
//! extended `G` and `H`, and `X` for the mouth at rest. A character's `mouth`
//! part may carry art for any subset of them (`Part::mouth_shapes`); a shape
//! that is missing is replaced by the nearest similar one from
//! [`MouthShape::similar`], then by `X`, then by the part's default art.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use crate::math::Color;
use crate::model::Art;

/// One of the nine mouth shapes. Written as its letter (`"A"`) in scenes and
/// in cue files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MouthShape {
    /// Closed lips: the sounds P, B, M.
    A,
    /// Slightly open, teeth clenched: K, S, T, and the long E.
    B,
    /// Open: the vowels of "bed" and "cat".
    C,
    /// Wide open: the vowel of "father".
    D,
    /// Rounded, slightly open: the vowels of "off" and "bird".
    E,
    /// Puckered: the sounds U, OO, W.
    F,
    /// Upper teeth on the lower lip: F, V (extended).
    G,
    /// Tongue raised behind the upper teeth: a long L (extended).
    H,
    /// At rest: nobody is speaking, or a pause (extended).
    X,
}

impl MouthShape {
    /// All shapes, in letter order.
    pub const ALL: [MouthShape; 9] = [
        MouthShape::A,
        MouthShape::B,
        MouthShape::C,
        MouthShape::D,
        MouthShape::E,
        MouthShape::F,
        MouthShape::G,
        MouthShape::H,
        MouthShape::X,
    ];

    /// The letter this shape is written as.
    pub fn letter(self) -> char {
        match self {
            MouthShape::A => 'A',
            MouthShape::B => 'B',
            MouthShape::C => 'C',
            MouthShape::D => 'D',
            MouthShape::E => 'E',
            MouthShape::F => 'F',
            MouthShape::G => 'G',
            MouthShape::H => 'H',
            MouthShape::X => 'X',
        }
    }

    /// The shape for an upper-case letter `A`-`H` or `X`; nothing else.
    pub fn from_letter(c: char) -> Option<MouthShape> {
        MouthShape::ALL.into_iter().find(|s| s.letter() == c)
    }

    /// The shape for a one-letter text such as `"B"`.
    pub fn parse(s: &str) -> Option<MouthShape> {
        let mut chars = s.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => MouthShape::from_letter(c),
            _ => None,
        }
    }

    /// What the shape looks like, for the artist's table.
    pub fn description(self) -> &'static str {
        match self {
            MouthShape::A => "closed lips (P, B, M)",
            MouthShape::B => "slightly open, teeth clenched (K, S, T, long E)",
            MouthShape::C => "open (the vowels of \"bed\", \"cat\")",
            MouthShape::D => "wide open (the vowel of \"father\")",
            MouthShape::E => "rounded, slightly open (the vowels of \"off\", \"bird\")",
            MouthShape::F => "puckered (U, OO, W)",
            MouthShape::G => "upper teeth on the lower lip (F, V)",
            MouthShape::H => "tongue raised (a long L)",
            MouthShape::X => "rest (pause, silence)",
        }
    }

    /// The shapes that look most like this one, nearest first. This is the
    /// fixed fallback table: when a rig has no art for a shape, the first of
    /// these it does have is drawn. (`X` and then the part's default art come
    /// after the table; see [`MouthShape::fallbacks`].)
    pub fn similar(self) -> &'static [MouthShape] {
        use MouthShape::{A, B, C, D, E, F, G, H, X};
        match self {
            A => &[X, B],
            B => &[C, A],
            C => &[B, D],
            D => &[C, E],
            E => &[F, C],
            F => &[E, C],
            G => &[B, C],
            H => &[C, D],
            X => &[A],
        }
    }

    /// The order in which a rig looks for art: the shape itself, the similar
    /// ones, then `X`. After the last the caller uses the default art.
    pub fn fallbacks(self) -> impl Iterator<Item = MouthShape> {
        std::iter::once(self)
            .chain(self.similar().iter().copied())
            .chain(std::iter::once(MouthShape::X))
    }
}

impl fmt::Display for MouthShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.letter())
    }
}

impl Serialize for MouthShape {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut buf = [0u8; 4];
        s.serialize_str(self.letter().encode_utf8(&mut buf))
    }
}

impl<'de> Deserialize<'de> for MouthShape {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<MouthShape, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = MouthShape;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a mouth shape: one of A B C D E F G H X, written as a string")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<MouthShape, E> {
                MouthShape::parse(v).ok_or_else(|| {
                    E::custom(format!(
                        "unknown mouth shape {v:?} (the shapes are A B C D E F G H X)"
                    ))
                })
            }
        }
        d.deserialize_str(V)
    }
}

// ---------------------------------------------------------------------------
// Placeholder mouths
// ---------------------------------------------------------------------------

/// Colour of the teeth in the placeholder mouths.
pub const TEETH: Color = Color::rgb(0xee, 0xe6, 0xd6);
/// Colour of the tongue in the placeholder mouths.
pub const TONGUE: Color = Color::rgb(0xc4, 0x62, 0x5c);

/// Simple geometric mouths for every shape, for a placeholder rig whose
/// resting mouth is `width` x `height` stage units in `color`: a thin line at
/// rest, a firm line for closed lips, a teeth bar, a small and a wide oval, a
/// rounded "O", a small pucker, a short teeth bar for F and V, a tongue oval
/// for L. Each is one shape centred on the part's origin, so it replaces the
/// resting art without moving the joint.
pub fn placeholder_mouth_shapes(
    width: f32,
    height: f32,
    color: Color,
) -> BTreeMap<MouthShape, Art> {
    let oval = |w: f32, h: f32, color: Color| Art::RoundRect {
        w,
        h,
        radius: w.min(h) / 2.0,
        color,
    };
    let bar = |w: f32, h: f32, color: Color| Art::RoundRect {
        w,
        h,
        radius: w.min(h) / 3.0,
        color,
    };
    BTreeMap::from([
        (MouthShape::A, bar(width, height * 0.5, color)),
        (MouthShape::B, bar(width, height * 0.95, TEETH)),
        (MouthShape::C, oval(width * 0.78, height * 1.7, color)),
        (MouthShape::D, oval(width * 0.95, height * 2.8, color)),
        (MouthShape::E, oval(width * 0.55, height * 2.2, color)),
        (MouthShape::F, oval(width * 0.32, height * 1.3, color)),
        (MouthShape::G, bar(width * 0.72, height * 0.95, TEETH)),
        (MouthShape::H, oval(width * 0.72, height * 1.5, TONGUE)),
        (MouthShape::X, bar(width * 0.85, height * 0.32, color)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_round_trip_and_only_the_nine_exist() {
        for s in MouthShape::ALL {
            assert_eq!(MouthShape::from_letter(s.letter()), Some(s));
            assert_eq!(MouthShape::parse(&s.to_string()), Some(s));
        }
        for bad in ["", "a", "I", "Z", "AB", " A", "0", "x"] {
            assert_eq!(MouthShape::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn every_shape_has_a_table_entry_that_never_points_at_itself() {
        for s in MouthShape::ALL {
            assert!(!s.similar().is_empty(), "{s}");
            assert!(!s.similar().contains(&s), "{s} lists itself");
            assert!(
                !s.description().is_empty(),
                "{s} has no description for the artist"
            );
        }
    }

    #[test]
    fn fallbacks_start_with_the_shape_and_end_with_x() {
        for s in MouthShape::ALL {
            let v: Vec<_> = s.fallbacks().collect();
            assert_eq!(v[0], s);
            assert_eq!(*v.last().unwrap(), MouthShape::X);
        }
        let d: Vec<_> = MouthShape::D.fallbacks().collect();
        assert_eq!(
            d,
            [MouthShape::D, MouthShape::C, MouthShape::E, MouthShape::X]
        );
    }

    #[test]
    fn placeholder_mouths_cover_all_nine_and_differ() {
        let m = placeholder_mouth_shapes(30.0, 9.0, Color::rgb(0x7a, 0x2e, 0x2e));
        assert_eq!(m.len(), 9);
        for a in MouthShape::ALL {
            for b in MouthShape::ALL {
                if a != b {
                    assert_ne!(m[&a], m[&b], "{a} and {b} look alike");
                }
            }
        }
    }
}
