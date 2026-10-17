//! [`Decimal`]: a number written with up to three decimals in a RON file,
//! stored as an integer count of thousandths.
//!
//! Game data wants to say `speed: 1.2`. A float in the data would be a hazard
//! (two machines could disagree about the last bit), so the value is turned into
//! an integer of thousandths the moment it is read and refused if that loses
//! anything (`0.0004`, `1.0000000001`, `NaN`). From there on it is integer
//! arithmetic only: [`Decimal::to_fx`] makes the `sf_math` fixed-point number,
//! [`Decimal::to_fx_per_tick`] a rate per 20 Hz tick, and
//! [`Decimal::to_ticks`] a whole number of ticks.
//!
//! The decimals are counted on the **text** of the number (`FromStr` is exact).
//! The RON reader hands a visitor an `f64`; this file turns it back into its
//! shortest text, which is the digits that were written (for any number with at
//! most 15 significant digits), and parses that. On top of this, the file
//! loaders scan the raw text of a data file for number literals with more than
//! three decimals (`Source::check_numbers`), so even a literal with too many
//! digits to survive the `f64` is refused, with its line number. This file is
//! the only place in the unit definitions where a float type is named.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};
use sf_math::{Fx, TICK_MILLIS as TICK_MILLIS_U32, TICKS_PER_SECOND as TICKS_PER_SECOND_U32};

/// Largest accepted magnitude, in whole units (a speed, a range or a number
/// of seconds beyond a million is a typo).
const MAX_WHOLE: i64 = 1_000_000;

/// Milliseconds of game time per tick (20 Hz, B3): `sf_math::TICK_MILLIS`.
pub const TICK_MILLIS: i64 = TICK_MILLIS_U32 as i64;

/// Ticks per game second (B3): `sf_math::TICKS_PER_SECOND`.
pub const TICKS_PER_SECOND: u32 = TICKS_PER_SECOND_U32;

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

    /// A rate per game second (cells per second) as a rate per tick, rounded
    /// **once**: `value / (1000 * 20)` to the nearest raw unit, ties away from
    /// zero. Going through [`Decimal::to_fx`] and dividing that by 20 rounds
    /// twice and is off by one raw unit for about 2.5 % of all values (1.1
    /// cells per second gives 3605 that way, and 3604 is right).
    pub const fn to_fx_per_tick(self) -> Fx {
        Fx::from_ratio(self.0, 1000 * TICKS_PER_SECOND as i64)
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

/// Why a text is not a [`Decimal`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseDecimalError(String);

impl fmt::Display for ParseDecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseDecimalError {}

impl FromStr for Decimal {
    type Err = ParseDecimalError;

    /// Parses `-?digits(.digits)?` with at most three decimals and a whole part
    /// of at most a million, exactly. An exponent, a `+` sign, white space or a
    /// fourth decimal (even a zero) is refused.
    fn from_str(text: &str) -> Result<Decimal, ParseDecimalError> {
        let err = |why: &str| ParseDecimalError(format!("\"{text}\": {why}"));
        let (negative, body) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, frac) = body.split_once('.').unwrap_or((body, ""));
        let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
        if whole.is_empty() || !digits(whole) || !digits(frac) {
            return Err(err("not a plain decimal number"));
        }
        if body.contains('.') && frac.is_empty() {
            return Err(err("nothing after the decimal point"));
        }
        if frac.len() > 3 {
            return Err(err("more than three decimals"));
        }
        let whole_value: i64 = match whole.parse() {
            Ok(n) if n <= MAX_WHOLE => n,
            _ => return Err(err(&format!("out of range (at most {MAX_WHOLE})"))),
        };
        let mut milli = whole_value * 1000;
        if !frac.is_empty() {
            let scale = 10_i64.pow(3 - frac.len() as u32);
            milli += frac.parse::<i64>().map_err(|_| err("not a number"))? * scale;
        }
        Ok(Decimal(if negative { -milli } else { milli }))
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
        if !v.is_finite() {
            return Err(E::custom(format!("{v} is not a usable number")));
        }
        // `{}` prints the shortest text that reads back as the same `f64` and
        // never an exponent: the text that was written, whenever it had at
        // most 15 significant digits. The decimals are counted on that text.
        format!("{v}")
            .parse::<Decimal>()
            .map_err(|e| E::custom(e.to_string()))
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
            parse("1.2345")
                .unwrap_err()
                .contains("more than three decimals")
        );
        assert!(parse("99999999").unwrap_err().contains("out of range"));
        assert!(parse("1e30").is_err());
    }

    #[test]
    fn the_decimals_are_counted_on_the_text() {
        // The old float tolerance (1e-6 around a whole thousandth) let the
        // first of these in.
        for text in ["1.0000000001", "0.0004", "2.0001", "0.1234567"] {
            let reader = parse(text).unwrap_err();
            assert!(
                reader.contains("more than three decimals"),
                "{text}: {reader}"
            );
            assert!(text.parse::<Decimal>().is_err(), "{text}");
        }
        // A trailing zero is a fourth decimal too. The RON reader cannot see
        // it (it hands over 1.0); the file scan does, see `Source::check_numbers`.
        assert!("1.0000".parse::<Decimal>().is_err());
        assert_eq!("12.345".parse::<Decimal>(), Ok(Decimal(12345)));
        assert_eq!("0.001".parse::<Decimal>(), Ok(Decimal(1)));
        assert_eq!("-0.5".parse::<Decimal>(), Ok(Decimal(-500)));
        assert_eq!("7".parse::<Decimal>(), Ok(Decimal(7000)));
        for bad in ["", ".5", "5.", "+1", "1e3", " 1", "1.2.3", "--1", "1,5"] {
            assert!(bad.parse::<Decimal>().is_err(), "{bad:?}");
        }
        // Every value of the thousandths grid survives the trip through the
        // reader: text to f64 to text.
        for milli in -20_000..=20_000 {
            let text = Decimal(milli).to_string();
            assert_eq!(parse(&text), Ok(Decimal(milli)), "{text}");
        }
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
