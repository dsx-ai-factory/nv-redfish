// SPDX-FileCopyrightText: Copyright (c) 2025 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! `Edm.Duration` primitive wrapper
//!
//! Represents ISO 8601 durations as used by OData/Redfish via `Edm.Duration`.
//! Internally uses `rust_decimal::Decimal` seconds to preserve precision, supports
//! negative values and fractional seconds, and displays in canonical
//! `[-]P[nD][T[nH][nM]nS]` form.
//!
//! References:
//! - OASIS OData 4.01 CSDL, Primitive Types: Edm.Duration — see `Part 3: CSDL`
//!   (`https://docs.oasis-open.org/odata/odata/v4.01/odata-v4.01-part3-csdl.html`).
//! - DMTF Redfish Specification DSP0266 (`https://www.dmtf.org/standards/redfish`).
//!
//! Examples
//! ```rust
//! use nv_redfish_core::EdmDuration;
//! use std::str::FromStr;
//!
//! let d = EdmDuration::from_str("PT1H2M3.5S").unwrap();
//! assert_eq!(d.to_string(), "PT1H2M3.5S");
//! assert!((d.as_f64_seconds() - 3723.5).abs() < f64::EPSILON);
//! ```
//!
//! ```rust
//! use nv_redfish_core::{edm_duration::Error, EdmDuration};
//! use std::convert::TryFrom;
//! use std::time::Duration;
//!
//! let convert = |input: &str| Duration::try_from(input.parse::<EdmDuration>().unwrap());
//! let large = convert("PT20000000000.000000001S").unwrap();
//! assert_eq!(large, Duration::new(20_000_000_000, 1));
//! assert_eq!(convert("P1D").unwrap(), Duration::from_secs(86_400));
//! assert_eq!(convert("PT0S").unwrap(), Duration::ZERO);
//! assert_eq!(convert("PT1.0000000005S").unwrap(), Duration::from_secs(1));
//! assert_eq!(convert("PT1.9999999995S").unwrap(), Duration::from_secs(2));
//! assert!(matches!(convert("-PT1S"), Err(Error::CannotConvertNegativeEdmDuration)));
//! assert!(matches!(convert("PT18446744073709551616S"), Err(Error::ValueTooBig)));
//! ```

use rust_decimal::prelude::ToPrimitive as _;
use rust_decimal::Decimal;
use serde::de::Error as DeError;
use serde::de::Visitor;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::convert::TryFrom;
use std::error::Error as StdError;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::str::Chars;
use std::str::FromStr;
use std::time::Duration as StdDuration;

/// `EdmDuration` represented by Edm.EdmDuration type.
///
/// This type designed to prevent data loss during deserialization and
/// provides conversion to specific data types. If you don't care
/// about precision you can always use conversion to f64 seconds.
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct EdmDuration(Decimal);

impl EdmDuration {
    /// Convert to seconds represented as f64. Note that this function
    /// may return +Inf or -Inf if number outside of f64 range.
    #[must_use]
    pub fn as_f64_seconds(&self) -> f64 {
        Self::decimal_to_f64_lossy(self.0)
    }

    /// Extract seconds represented be `Decimal` from `EdmDuration`.
    #[must_use]
    pub const fn as_decimal(&self) -> Decimal {
        self.0
    }

    fn take_digits<'a>(chars: &Chars<'a>) -> (&'a str, Option<char>, Chars<'a>) {
        let s = chars.as_str();
        for (i, ch) in s.char_indices() {
            if ch.is_ascii_digit() || ch == '.' {
                continue;
            }
            let digits = &s[..i];
            let rest = &s[i + ch.len_utf8()..];
            return (digits, Some(ch), rest.chars());
        }
        (s, None, "".chars())
    }

    fn decimal_to_f64_lossy(d: Decimal) -> f64 {
        d.to_f64().unwrap_or_else(|| {
            if d.is_sign_negative() {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        })
    }

    fn div_with_reminder(v: Decimal, d: Decimal) -> (Decimal, Decimal) {
        let reminder = v % d;
        ((v - reminder) / d, reminder)
    }
}

/// Errors of `EdmDuration`.
#[derive(Debug)]
pub enum Error {
    /// Invalid Edm.Duration string.
    InvalidEdmDuration(String),
    /// Data cannot be represented by internal type.
    Overflow(String),
    /// Cannot convert negative Edm.Duration to standard duration,
    CannotConvertNegativeEdmDuration,
    /// Value of Edm.Duration is too big to be represented by standard
    /// duration.
    ValueTooBig,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::InvalidEdmDuration(v) => write!(f, "invalid duration: {v}"),
            Self::Overflow(v) => write!(f, "invalid duration: number overflow: {v}"),
            Self::CannotConvertNegativeEdmDuration => "cannot convert negative duration".fmt(f),
            Self::ValueTooBig => "duration: value to big".fmt(f),
        }
    }
}

impl StdError for Error {}

/// Converts to the nearest nanosecond, with ties rounded to even.
///
/// # Errors
///
/// Returns [`Error::CannotConvertNegativeEdmDuration`] for negative durations and
/// [`Error::ValueTooBig`] when the rounded duration exceeds [`std::time::Duration::MAX`].
impl TryFrom<EdmDuration> for StdDuration {
    type Error = Error;

    fn try_from(duration: EdmDuration) -> Result<Self, Error> {
        const NANOSECOND_DECIMAL_PLACES: u32 = 9;
        const NANOS_PER_SECOND: u32 = 10_u32.pow(NANOSECOND_DECIMAL_PLACES);

        if duration.0.is_sign_negative() {
            return Err(Error::CannotConvertNegativeEdmDuration);
        }

        let whole_seconds = duration.0.to_u64().ok_or(Error::ValueTooBig)?;

        let fractional_seconds = duration.0.fract().round_dp(NANOSECOND_DECIMAL_PLACES);

        let nanoseconds = (fractional_seconds * Decimal::from(NANOS_PER_SECOND))
            .to_u64()
            .ok_or(Error::ValueTooBig)?;

        Self::from_secs(whole_seconds)
            .checked_add(Self::from_nanos(nanoseconds))
            .ok_or(Error::ValueTooBig)
    }
}

impl FromStr for EdmDuration {
    type Err = Error;
    fn from_str(v: &str) -> Result<Self, Error> {
        let mut chars = v.chars();
        let make_err = || Error::InvalidEdmDuration(v.into());
        let overflow_err = || Error::Overflow(v.into());
        let maybe_sign = chars.next().ok_or_else(make_err)?;
        let (neg, p) = if maybe_sign == '-' {
            (Decimal::NEGATIVE_ONE, chars.next().ok_or_else(make_err)?)
        } else {
            (Decimal::ONE, maybe_sign)
        };
        (p == 'P').then_some(()).ok_or_else(make_err)?;

        let to_decimal = |val: &str, mul| {
            Decimal::from_str_exact(val)
                .map(|d| d.checked_mul(Decimal::from(mul)).ok_or_else(&overflow_err))
                .map_err(|_| make_err())
                .flatten()
        };

        let mut result = Decimal::ZERO;
        let (val, maybe_next, mut chars) = Self::take_digits(&chars);
        match maybe_next {
            Some('T') => (),
            Some('D') => match chars.next() {
                Some('T') => {
                    result = result
                        .checked_add(to_decimal(val, 3600 * 24)?)
                        .ok_or_else(overflow_err)?;
                }
                None => return to_decimal(val, 3600 * 24).map(|v| Self(v * neg)),
                _ => Err(make_err())?,
            },
            _ => Err(make_err())?,
        }

        loop {
            let (val, maybe_next, new_chars) = Self::take_digits(&chars);
            chars = new_chars;
            let mul = match maybe_next {
                Some('H') => 3600,
                Some('M') => 60,
                Some('S') => 1,
                Some(_) => Err(make_err())?,
                None => break,
            };
            result = result
                .checked_add(to_decimal(val, mul)?)
                .ok_or_else(overflow_err)?;
        }
        Ok(Self(result * neg))
    }
}

impl Display for EdmDuration {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        if self.0 == Decimal::ZERO {
            // Normalize zero to a canonical representation
            return write!(f, "PT0S");
        }
        let value = if self.0.is_sign_negative() {
            write!(f, "-P")?;
            -self.0
        } else {
            write!(f, "P")?;
            self.0
        };
        let (days, value) = Self::div_with_reminder(value, Decimal::from(24 * 3600));
        if days != Decimal::ZERO {
            write!(f, "{}D", days.normalize())?;
        }
        if value != Decimal::ZERO {
            write!(f, "T")?;
            let (hours, value) = Self::div_with_reminder(value, Decimal::from(3600));
            if hours != Decimal::ZERO {
                write!(f, "{}H", hours.normalize())?;
            }
            let (mins, value) = Self::div_with_reminder(value, Decimal::from(60));
            if mins != Decimal::ZERO {
                write!(f, "{}M", mins.normalize())?;
            }
            write!(f, "{}S", value.normalize())?;
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for EdmDuration {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct ValVisitor {}
        impl Visitor<'_> for ValVisitor {
            type Value = EdmDuration;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> FmtResult {
                formatter.write_str("Edm.Duration string")
            }
            fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(DeError::custom)
            }
        }

        de.deserialize_string(ValVisitor {})
    }
}

impl Serialize for EdmDuration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_string().serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str_exact(s).unwrap()
    }

    #[test]
    fn parses_time_only_hms() {
        let d = EdmDuration::from_str("PT1H2M3S").unwrap();
        assert_eq!(d.0, Decimal::from(3600 + 120 + 3));
    }

    #[test]
    fn parses_day_only() {
        let d = EdmDuration::from_str("P3D").unwrap();
        assert_eq!(d.0, Decimal::from(3 * 86400));
    }

    #[test]
    fn parses_day_and_time() {
        let d = EdmDuration::from_str("P1DT1H").unwrap();
        assert_eq!(d.0, Decimal::from(86400 + 3600));
    }

    #[test]
    fn parses_fractional_seconds() {
        let d = EdmDuration::from_str("PT0.25S").unwrap();
        assert_eq!(d.0, dec("0.25"));
    }

    #[test]
    fn parses_fractional_minutes_and_days() {
        let d1 = EdmDuration::from_str("PT1.5M").unwrap();
        assert_eq!(d1.0, Decimal::from(90));

        let d2 = EdmDuration::from_str("P1.5D").unwrap();
        assert_eq!(d2.0, Decimal::from(129600));
    }

    #[test]
    fn parses_negative_durations() {
        let d1 = EdmDuration::from_str("-PT2M").unwrap();
        assert_eq!(d1.0, Decimal::from(-120));

        let d2 = EdmDuration::from_str("-P1D").unwrap();
        assert_eq!(d2.0, Decimal::from(-86400));
    }

    #[test]
    fn parses_zero_variants() {
        let d1 = EdmDuration::from_str("PT0S").unwrap();
        assert_eq!(d1.0, Decimal::from(0));

        let d2 = EdmDuration::from_str("PT").unwrap();
        assert_eq!(d2.0, Decimal::from(0));
    }

    #[test]
    fn rejects_malformed_inputs() {
        assert!(EdmDuration::from_str("").is_err());
        assert!(EdmDuration::from_str("P").is_err());
        assert!(EdmDuration::from_str("T1H").is_err());
        assert!(EdmDuration::from_str("PT1X").is_err());
        assert!(EdmDuration::from_str("-P").is_err());
        assert!(EdmDuration::from_str("P1000000000000000000000000000D").is_err());
    }

    #[test]
    fn formats_zero_duration() {
        let d = EdmDuration::from_str("PT").unwrap();
        assert_eq!(format!("{}", d), "PT0S");
    }

    #[test]
    fn formats_seconds_only() {
        let d = EdmDuration::from_str("PT3S").unwrap();
        assert_eq!(format!("{}", d), "PT3S");
    }

    #[test]
    fn formats_fractional_seconds() {
        let d = EdmDuration::from_str("PT0.25S").unwrap();
        assert_eq!(format!("{}", d), "PT0.25S");
    }

    #[test]
    fn formats_minutes_and_hours_with_zero_seconds() {
        let d1 = EdmDuration::from_str("PT2M").unwrap();
        assert_eq!(format!("{}", d1), "PT2M0S");

        let d2 = EdmDuration::from_str("PT1H").unwrap();
        assert_eq!(format!("{}", d2), "PT1H0S");

        let d3 = EdmDuration::from_str("PT1H2M").unwrap();
        assert_eq!(format!("{}", d3), "PT1H2M0S");
    }

    #[test]
    fn formats_days_only_and_day_time() {
        let d1 = EdmDuration::from_str("P3D").unwrap();
        assert_eq!(format!("{}", d1), "P3D");

        let d2 = EdmDuration::from_str("P1DT1H").unwrap();
        assert_eq!(format!("{}", d2), "P1DT1H0S");
    }

    #[test]
    fn formats_negative_durations() {
        let d1 = EdmDuration::from_str("-PT2M").unwrap();
        assert_eq!(format!("{}", d1), "-PT2M0S");

        let d2 = EdmDuration::from_str("-P1D").unwrap();
        assert_eq!(format!("{}", d2), "-P1D");
    }

    #[test]
    fn normalizes_fractional_minutes_on_display() {
        let d = EdmDuration::from_str("PT1.5M").unwrap();
        assert_eq!(format!("{}", d), "PT1M30S");
    }

    #[test]
    fn formats_trims_trailing_zero_seconds() {
        let d = EdmDuration::from_str("PT30.0S").unwrap();
        // Desired: trailing .0 trimmed
        assert_eq!(format!("{}", d), "PT30S");
    }

    #[test]
    fn formats_leading_zero_inputs() {
        let d = EdmDuration::from_str("PT01S").unwrap();
        assert_eq!(format!("{}", d), "PT1S");
    }

    #[test]
    fn formats_large_hours_breakdown() {
        // 100_000 hours = 4166 days and 16 hours
        let d = EdmDuration::from_str("PT100000H").unwrap();
        assert_eq!(format!("{}", d), "P4166DT16H0S");
    }

    #[test]
    fn formats_fractional_hours() {
        let d = EdmDuration::from_str("PT1.75H").unwrap();
        assert_eq!(format!("{}", d), "PT1H45M0S");
    }

    #[test]
    fn formats_fractional_days() {
        let d = EdmDuration::from_str("P1.25D").unwrap();
        assert_eq!(format!("{}", d), "P1DT6H0S");
    }

    #[test]
    fn formats_minute_and_hour_carry_from_seconds() {
        let d1 = EdmDuration::from_str("PT60S").unwrap();
        assert_eq!(format!("{}", d1), "PT1M0S");

        let d2 = EdmDuration::from_str("PT3600S").unwrap();
        assert_eq!(format!("{}", d2), "PT1H0S");
    }

    #[test]
    fn formats_trims_excess_zero_fraction() {
        let d = EdmDuration::from_str("PT1.2300S").unwrap();
        assert_eq!(format!("{}", d), "PT1.23S");
    }

    #[test]
    fn test_exact_division() {
        let (q, r) = EdmDuration::div_with_reminder(Decimal::new(10, 0), Decimal::new(5, 0));
        assert_eq!(q, Decimal::new(2, 0));
        assert_eq!(r, Decimal::new(0, 0));
    }

    #[test]
    fn test_positive_non_exact() {
        let (q, r) = EdmDuration::div_with_reminder(Decimal::new(10, 0), Decimal::new(4, 0));
        assert_eq!(q, Decimal::new(2, 0));
        assert_eq!(r, Decimal::new(2, 0));
    }

    #[test]
    fn test_non_integer_division() {
        let v = Decimal::new(105, 1); // 10.5
        let d = Decimal::new(4, 0); // 4
        let (q, r) = EdmDuration::div_with_reminder(v, d);
        assert_eq!(q, Decimal::new(2, 0));
        assert_eq!(r, Decimal::new(25, 1)); // 2.5
    }

    #[test]
    fn test_zero_dividend() {
        let (q, r) = EdmDuration::div_with_reminder(Decimal::new(0, 0), Decimal::new(5, 0));
        assert_eq!(q, Decimal::new(0, 0));
        assert_eq!(r, Decimal::new(0, 0));
    }
}
