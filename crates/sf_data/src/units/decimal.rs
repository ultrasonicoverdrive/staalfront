//! [`Decimal`]: a number written with up to three decimals in a RON file,
//! stored as an integer count of thousandths.
//!
//! Game data wants to say `speed: 1.2`. A float in the data would be a hazard
//! (two machines could disagree about the last bit), so the value is turned into
//! an integer of thousandths the moment it is read and refused if that loses
//! anything (`0.0004`, `NaN`). From there on it is integer arithmetic only:
//! [`Decimal::to_fx`] makes the `sf_math` fixed-point number, and
//! [`Decimal::to_ticks`] a whole number of 20 Hz ticks.
//!
//! This file is the only place in the unit definitions where a float type is
//! named, and only to read the text of the data file.

use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};
use sf_math::Fx;

/// Largest accepted magnitude, in whole units (a speed, a range or a number
/// of seconds beyond a million is a typo).
const MAX_WHOLE: i64 = 1_000_000;

/// Milliseconds of game time per tick (20 Hz, B3). The same number as
/// `sf_sim::TICK_MILLIS`; a test in `tests/units.rs` keeps them equal.
pub const TICK_MILLIS: i64 = 50;

/// Ticks per game second (B3). Mirrors `sf_sim::TICKS_PER_SECOND`.
pub const TICKS_PER_SECOND: u32 = 20;

/// A decimal number with at most three decimals, as thousandths.
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Decimal(i64);

impl Decimal {
    /// Zero.
    pub const ZERO: Decimal = Decimal(0);

    /// From a count of thousandths (`1200` is `1.2`).
    pub const fn from_milli(milli: i64) -> Decimal {
        Decimal(milli)
    }

    /// The number as thousandths.
    pub const fn milli(self) -> i64 {
        self.0
    }

    /// `self == 0`.
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// `self < 0`.
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// The fixed-point number with the same value (rounded to nearest at
    /// 1/65536, ties away from zero, by `Fx::from_ratio`).
    pub const fn to_fx(self) -> Fx {
        Fx::from_ratio(self.0, 1000)
    }

    /// A duration in game seconds as whole ticks, or `None` if it is negative
    /// or not an exact multiple of one tick (0.05 s). Silent rounding would
    /// make a typo (`0.12`) look like a design choice.
    pub const fn to_ticks(self) -> Option<u32> {
        if self.0 < 0 || self.0 % TICK_MILLIS != 0 {
            return None;
        }
        let ticks = self.0 / TICK_MILLIS;
        if ticks > u32::MAX as i64 {
            None
        } else {
            Some(ticks as u32)
        }
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        let (whole, frac) = (abs / 1000, abs % 1000);
        if frac == 0 {
            write!(f, "{sign}{whole}")
        } else {
            let text = format!("{frac:03}");
            write!(f, "{sign}{whole}.{}", text.trim_end_matches('0'))
        }
    }
}

struct DecimalVisitor;

impl DecimalVisitor {
    fn whole<E: de::Error>(n: i64) -> Result<Decimal, E> {
        if n.unsigned_abs() > MAX_WHOLE as u64 {
            return Err(E::custom(format!(
                "{n} is out of range (at most {MAX_WHOLE})"
            )));
        }
        Ok(Decimal(n * 1000))
    }
}

impl Visitor<'_> for DecimalVisitor {
    type Value = Decimal;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a number with at most three decimals")
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Decimal, E> {
        Self::whole(v)
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Decimal, E> {
        match i64::try_from(v) {
            Ok(n) => Self::whole(n),
            Err(_) => Err(E::custom(format!("{v} is out of range"))),
        }
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Decimal, E> {
        if !v.is_finite() || v.abs() > MAX_WHOLE as f64 {
            return Err(E::custom(format!("{v} is not a usable number")));
        }
        let scaled = v * 1000.0;
        let rounded = scaled.round();
        // A literal with three decimals scaled by 1000 lands within a few
        // billionths of a whole number; anything further has more decimals.
        if (scaled - rounded).abs() > 1e-6 {
            return Err(E::custom(format!("{v} has more than three decimals")));
        }
        Ok(Decimal(rounded as i64))
    }
}

impl<'de> Deserialize<'de> for Decimal {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Decimal, D::Error> {
        d.deserialize_any(DecimalVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Decimal, String> {
        ron::from_str::<Decimal>(text).map_err(|e| e.to_string())
    }

    #[test]
    fn reads_integers_and_decimals() {
        assert_eq!(parse("4"), Ok(Decimal(4000)));
        assert_eq!(parse("1.2"), Ok(Decimal(1200)));
        assert_eq!(parse("0.7"), Ok(Decimal(700)));
        assert_eq!(parse("0.05"), Ok(Decimal(50)));
        assert_eq!(parse("12.345"), Ok(Decimal(12345)));
        assert_eq!(parse("-0.5"), Ok(Decimal(-500)));
    }

    #[test]
    fn refuses_what_it_cannot_hold_exactly() {
        assert!(
            parse("0.0004")
                .unwrap_err()
                .contains("more than three decimals")
        );
        assert!(
            parse("1.2345")
                .unwrap_err()
                .contains("more than three decimals")
        );
        assert!(parse("99999999").unwrap_err().contains("out of range"));
        assert!(parse("1e30").is_err());
    }

    #[test]
    fn fixed_point_and_ticks() {
        assert_eq!(Decimal(1200).to_fx(), Fx::from_ratio(6, 5));
        assert_eq!(Decimal(4000).to_fx(), Fx::from_int(4));
        assert_eq!(Decimal(1000).to_ticks(), Some(20));
        assert_eq!(Decimal(50).to_ticks(), Some(1));
        assert_eq!(Decimal(0).to_ticks(), Some(0));
        assert_eq!(Decimal(120).to_ticks(), None);
        assert_eq!(Decimal(-50).to_ticks(), None);
    }

    #[test]
    fn displays_without_trailing_zeros() {
        assert_eq!(Decimal(1200).to_string(), "1.2");
        assert_eq!(Decimal(4000).to_string(), "4");
        assert_eq!(Decimal(-50).to_string(), "-0.05");
    }
}
