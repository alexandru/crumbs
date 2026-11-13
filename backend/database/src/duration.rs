//! Validated duration values.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use crumbs_derive::Value;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::nutrition_model;

/// A positive duration with nanosecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Value)]
pub struct PositiveDuration(Duration);

impl PositiveDuration {
    fn parse(value: &str) -> Result<Self, DurationError> {
        let invalid = || DurationError::Format;
        let (number, unit) = value
            .trim()
            .rsplit_once(char::is_whitespace)
            .ok_or_else(invalid)?;
        let factor: u128 = match unit {
            "s" => 1_000_000_000,
            "min" => 60_000_000_000,
            "h" => 3_600_000_000_000,
            _ => return Err(invalid()),
        };

        let decimal = nutrition_model::parse_decimal(number.trim()).map_err(|_| invalid())?;
        if decimal <= Decimal::ZERO {
            return Err(DurationError::NonPositive);
        }

        let mantissa = u128::try_from(decimal.mantissa()).map_err(|_| invalid())?;
        let divisor = 10_u128.pow(decimal.scale());
        // Reduce the fraction before multiplication to test nanosecond precision without rounding.
        let common = gcd(divisor, factor);
        let divisor = divisor / common;
        let factor = factor / common;
        if mantissa % divisor != 0 {
            return Err(DurationError::Precision);
        }

        let count = mantissa / divisor;
        if count > Duration::MAX.as_nanos() / factor {
            return Err(DurationError::Range);
        }

        let nanos = count * factor;
        let secs = u64::try_from(nanos / 1_000_000_000).map_err(|_| invalid())?;
        let subsec = u32::try_from(nanos % 1_000_000_000).map_err(|_| invalid())?;
        Self::try_from(Duration::new(secs, subsec))
    }
}

impl TryFrom<Duration> for PositiveDuration {
    type Error = DurationError;

    fn try_from(value: Duration) -> Result<Self, Self::Error> {
        if value.is_zero() {
            return Err(DurationError::NonPositive);
        }

        Ok(Self(value))
    }
}

impl FromStr for PositiveDuration {
    type Err = DurationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for PositiveDuration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seconds =
            Decimal::from(self.0.as_secs()) + Decimal::new(i64::from(self.0.subsec_nanos()), 9);
        write!(formatter, "{} s", seconds.normalize())
    }
}

impl Serialize for PositiveDuration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PositiveDuration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A duration is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DurationError {
    #[error("duration must be an exact decimal with unit s, min, or h")]
    Format,
    #[error("duration must be positive")]
    NonPositive,
    #[error("duration must have nanosecond precision")]
    Precision,
    #[error("duration is too large")]
    Range,
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

#[cfg(test)]
#[path = "../unit-tests/duration.rs"]
mod tests;
