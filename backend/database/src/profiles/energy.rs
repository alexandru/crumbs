use std::fmt;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::nutrition_model::{Kilocalories, QuantityError};

/// A nonnegative daily energy rate in kilocalories per day.
///
/// JSON represents this rate as a string with the `kcal` unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KilocaloriesPerDay(Decimal);

impl KilocaloriesPerDay {
    /// Creates a daily energy rate from an exact fixed-point value.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is negative.
    pub fn try_new(value: Decimal) -> Result<Self, QuantityError> {
        Kilocalories::try_new(value).map(|value| Self(value.value()))
    }

    /// Returns the exact fixed-point value in kilocalories per day.
    pub const fn value(self) -> Decimal {
        self.0
    }

    /// Adds two daily energy rates, or returns `None` on overflow.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }
}

impl FromStr for KilocaloriesPerDay {
    type Err = QuantityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<Kilocalories>()
            .map(|value| Self(value.value()))
    }
}

impl fmt::Display for KilocaloriesPerDay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} kcal", self.0)
    }
}

impl Serialize for KilocaloriesPerDay {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for KilocaloriesPerDay {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
