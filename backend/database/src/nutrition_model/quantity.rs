use std::fmt;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Invalid source measurements are rejected before entering the domain model.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QuantityError {
    #[error("invalid or incompatible measurement unit: {0}")]
    Unit(String),
    #[error("invalid decimal or unsupported precision/range: {0}")]
    Decimal(String),
    #[error("measurements must not be negative")]
    Negative,
    #[error("measurements must be positive")]
    NonPositive,
    #[error("a strict upper bound must be greater than zero")]
    ZeroStrictUpperBound,
    #[error("measurement arithmetic overflow or out of range")]
    Arithmetic,
}

/// A comparison or approximation supplied by the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Qualifier {
    #[default]
    Exact,
    LessThan,
    AtMost,
    GreaterThan,
    AtLeast,
    Approximate,
}

/// A nonnegative decimal measurement in its retained source unit.
pub trait Measurement: sealed::Sealed + Copy + fmt::Display {
    /// Returns the decimal value in the retained source unit.
    fn value(self) -> Decimal;

    /// Parses a decimal value and its unit without first formatting them together.
    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError>;

    /// Multiplies this measurement by a fixed-point factor while preserving its unit.
    ///
    /// Arithmetic uses at most 28 fractional decimal places and the `Decimal` 96-bit
    /// coefficient. When precision must be reduced, midpoint ties round to nearest even.
    ///
    /// # Errors
    ///
    /// Returns [`QuantityError::Arithmetic`] if the result is outside the representable
    /// decimal range, or [`QuantityError::Negative`] if the result is negative.
    fn checked_scale(self, factor: Decimal) -> Result<Self, QuantityError>;
}

/// A measured nutrient amount with its source-supplied qualification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reported<T: Measurement> {
    qualifier: Qualifier,
    amount: T,
}

impl<T: Measurement> Reported<T> {
    /// Creates a reported amount, rejecting an impossible strict upper bound.
    pub fn new(qualifier: Qualifier, amount: T) -> Result<Self, QuantityError> {
        if qualifier == Qualifier::LessThan && amount.value().is_zero() {
            return Err(QuantityError::ZeroStrictUpperBound);
        }
        Ok(Self { qualifier, amount })
    }

    /// Parses a qualified decimal value and its unit.
    pub fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
        let value = value.trim();
        let (qualifier, amount) = qualifier_and_amount(value);
        Self::new(qualifier, T::parse_with_unit(amount.trim(), unit)?)
    }

    /// Returns the source-supplied comparison or approximation.
    pub const fn qualifier(&self) -> Qualifier {
        self.qualifier
    }

    /// Returns the measured amount.
    pub const fn amount(&self) -> T {
        self.amount
    }

    /// Scales the amount while preserving its source qualifier.
    ///
    /// # Errors
    ///
    /// Returns [`QuantityError::Arithmetic`] if the result is outside the representable
    /// decimal range, [`QuantityError::Negative`] if the result is negative, or
    /// [`QuantityError::ZeroStrictUpperBound`] if a `LessThan` amount rounds to zero.
    pub fn checked_scale(self, factor: Decimal) -> Result<Self, QuantityError> {
        Self::new(self.qualifier, self.amount.checked_scale(factor)?)
    }
}

impl<T: Measurement> FromStr for Reported<T> {
    type Err = QuantityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (qualifier, rest) = qualifier_and_amount(value.trim());
        let (number, unit) = split_quantity(rest.trim())?;
        Self::new(qualifier, T::parse_with_unit(number, unit)?)
    }
}

impl<T: Measurement> fmt::Display for Reported<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.qualifier {
            Qualifier::Exact => "",
            Qualifier::LessThan => "< ",
            Qualifier::AtMost => "<= ",
            Qualifier::GreaterThan => "> ",
            Qualifier::AtLeast => ">= ",
            Qualifier::Approximate => "~ ",
        };
        write!(f, "{prefix}{}", self.amount)
    }
}

impl<T: Measurement> Serialize for Reported<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de, T: Measurement> Deserialize<'de> for Reported<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

// Each concrete unit owns its invariant; dimension enums cannot contain raw decimals.
macro_rules! measurement {
    ($name:ident, $unit:literal) => {
        #[doc = concat!("A nonnegative, exact decimal quantity in ", $unit, ".")]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(Decimal);

        impl $name {
            /// Creates a nonnegative quantity from an exact fixed-point value.
            pub fn try_new(value: Decimal) -> Result<Self, QuantityError> {
                if value.is_sign_negative() && !value.is_zero() {
                    return Err(QuantityError::Negative);
                }
                Ok(Self(value))
            }

            /// Returns the exact fixed-point value in this unit.
            pub const fn value(self) -> Decimal {
                self.0
            }
        }

        impl FromStr for $name {
            type Err = QuantityError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let (number, unit) = split_quantity(value)?;
                <Self as Measurement>::parse_with_unit(number, unit)
            }
        }

        impl sealed::Sealed for $name {}

        impl Measurement for $name {
            fn value(self) -> Decimal {
                self.0
            }

            fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
                if unit != $unit {
                    return Err(QuantityError::Unit(unit.to_owned()));
                }
                Self::try_new(parse_decimal(value)?)
            }

            fn checked_scale(self, factor: Decimal) -> Result<Self, QuantityError> {
                Self::try_new(checked_mul(self.0, factor)?)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{} {}", self.0, $unit)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}

measurement!(Grams, "g");
measurement!(Milligrams, "mg");
measurement!(Micrograms, "µg");
measurement!(Millilitres, "ml");
measurement!(Litres, "l");
measurement!(Kilocalories, "kcal");
measurement!(Kilojoules, "kJ");
measurement!(Centimetres, "cm");
measurement!(Kilograms, "kg");

/// A mass amount retaining the supplied supported unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mass {
    Kilograms(Kilograms),
    Grams(Grams),
    Milligrams(Milligrams),
    Micrograms(Micrograms),
}

impl Mass {
    /// Returns the decimal value in the retained source unit.
    pub fn value(self) -> Decimal {
        match self {
            Self::Kilograms(value) => value.value(),
            Self::Grams(value) => value.value(),
            Self::Milligrams(value) => value.value(),
            Self::Micrograms(value) => value.value(),
        }
    }

    /// Converts this mass to grams using exact metric unit factors.
    ///
    /// Arithmetic uses at most 28 fractional decimal places and the `Decimal` 96-bit
    /// coefficient. When precision must be reduced, midpoint ties round to nearest even.
    ///
    /// # Errors
    ///
    /// Returns [`QuantityError::Arithmetic`] if the result is outside the representable
    /// decimal range.
    pub fn to_grams(self) -> Result<Grams, QuantityError> {
        match self {
            Self::Kilograms(value) => {
                Grams::try_new(checked_mul(value.value(), Decimal::from(1000))?)
            }
            Self::Grams(value) => Ok(value),
            Self::Milligrams(value) => {
                Grams::try_new(checked_div(value.value(), Decimal::from(1000))?)
            }
            Self::Micrograms(value) => {
                Grams::try_new(checked_div(value.value(), Decimal::from(1_000_000))?)
            }
        }
    }
}

impl sealed::Sealed for Mass {}

impl Measurement for Mass {
    fn value(self) -> Decimal {
        Mass::value(self)
    }

    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
        match unit {
            "kg" => Ok(Self::Kilograms(Kilograms::parse_with_unit(value, unit)?)),
            "g" => Ok(Self::Grams(Grams::try_new(parse_decimal(value)?)?)),
            "mg" => Ok(Self::Milligrams(Milligrams::try_new(parse_decimal(
                value,
            )?)?)),
            "µg" => Ok(Self::Micrograms(Micrograms::try_new(parse_decimal(
                value,
            )?)?)),
            _ => Err(QuantityError::Unit(unit.to_owned())),
        }
    }

    fn checked_scale(self, factor: Decimal) -> Result<Self, QuantityError> {
        match self {
            Self::Kilograms(value) => Ok(Self::Kilograms(value.checked_scale(factor)?)),
            Self::Grams(value) => Ok(Self::Grams(value.checked_scale(factor)?)),
            Self::Milligrams(value) => Ok(Self::Milligrams(value.checked_scale(factor)?)),
            Self::Micrograms(value) => Ok(Self::Micrograms(value.checked_scale(factor)?)),
        }
    }
}

impl FromStr for Mass {
    type Err = QuantityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (number, unit) = split_quantity(value)?;
        <Self as Measurement>::parse_with_unit(number, unit)
    }
}

impl fmt::Display for Mass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kilograms(value) => value.fmt(f),
            Self::Grams(value) => value.fmt(f),
            Self::Milligrams(value) => value.fmt(f),
            Self::Micrograms(value) => value.fmt(f),
        }
    }
}

/// Energy retaining the selected source unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Energy {
    Kilocalories(Kilocalories),
    Kilojoules(Kilojoules),
}

impl Energy {
    /// Returns the decimal value in the retained source unit.
    pub fn value(self) -> Decimal {
        match self {
            Self::Kilocalories(value) => value.value(),
            Self::Kilojoules(value) => value.value(),
        }
    }

    /// Converts this energy to kilocalories using `1 kcal = 4.184 kJ`.
    ///
    /// Arithmetic uses at most 28 fractional decimal places and the `Decimal` 96-bit
    /// coefficient. When precision must be reduced, midpoint ties round to nearest even.
    ///
    /// # Errors
    ///
    /// Returns [`QuantityError::Arithmetic`] if the result is outside the representable
    /// decimal range.
    pub fn to_kilocalories(self) -> Result<Kilocalories, QuantityError> {
        match self {
            Self::Kilocalories(value) => Ok(value),
            Self::Kilojoules(value) => {
                Kilocalories::try_new(checked_div(value.value(), Decimal::new(4184, 3))?)
            }
        }
    }
}

impl sealed::Sealed for Energy {}

impl Measurement for Energy {
    fn value(self) -> Decimal {
        Energy::value(self)
    }

    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
        match unit {
            "kcal" => Ok(Self::Kilocalories(Kilocalories::try_new(parse_decimal(
                value,
            )?)?)),
            "kJ" => Ok(Self::Kilojoules(Kilojoules::try_new(parse_decimal(
                value,
            )?)?)),
            _ => Err(QuantityError::Unit(unit.to_owned())),
        }
    }

    fn checked_scale(self, factor: Decimal) -> Result<Self, QuantityError> {
        match self {
            Self::Kilocalories(value) => Ok(Self::Kilocalories(value.checked_scale(factor)?)),
            Self::Kilojoules(value) => Ok(Self::Kilojoules(value.checked_scale(factor)?)),
        }
    }
}

impl FromStr for Energy {
    type Err = QuantityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (number, unit) = split_quantity(value)?;
        <Self as Measurement>::parse_with_unit(number, unit)
    }
}

impl fmt::Display for Energy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kilocalories(value) => value.fmt(f),
            Self::Kilojoules(value) => value.fmt(f),
        }
    }
}

/// A mass or volume that retains its source unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasuredQuantity {
    Mass(Mass),
    Volume(Volume),
}

impl MeasuredQuantity {
    /// Returns the decimal value in the retained source unit.
    pub fn value(self) -> Decimal {
        match self {
            Self::Mass(value) => value.value(),
            Self::Volume(value) => value.value(),
        }
    }

    /// Returns the measured mass, when the serving has a mass measurement.
    pub const fn mass(self) -> Option<Mass> {
        match self {
            Self::Mass(mass) => Some(mass),
            Self::Volume(_) => None,
        }
    }

    /// Returns the measured volume, when the serving has a volume measurement.
    pub const fn volume(self) -> Option<Volume> {
        match self {
            Self::Volume(volume) => Some(volume),
            Self::Mass(_) => None,
        }
    }

    /// Multiplies this measured quantity by a factor while preserving its unit.
    ///
    /// Arithmetic uses at most 28 fractional decimal places and the `Decimal` 96-bit
    /// coefficient. When precision must be reduced, midpoint ties round to nearest even.
    ///
    /// # Errors
    ///
    /// Returns [`QuantityError::Arithmetic`] if the result is outside the representable
    /// decimal range, or [`QuantityError::Negative`] if the result is negative.
    pub fn checked_scale(self, factor: Decimal) -> Result<Self, QuantityError> {
        match self {
            Self::Mass(value) => Ok(Self::Mass(value.checked_scale(factor)?)),
            Self::Volume(value) => Ok(Self::Volume(value.checked_scale(factor)?)),
        }
    }
}

impl FromStr for MeasuredQuantity {
    type Err = QuantityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (number, unit) = split_quantity(value)?;
        match unit {
            "ml" | "l" => Ok(Self::Volume(Volume::parse_with_unit(number, unit)?)),
            _ => Ok(Self::Mass(Mass::parse_with_unit(number, unit)?)),
        }
    }
}

/// A volume amount retaining the supplied supported unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Volume {
    Millilitres(Millilitres),
    Litres(Litres),
}

impl Volume {
    /// Returns the decimal value in the retained source unit.
    pub fn value(self) -> Decimal {
        match self {
            Self::Millilitres(value) => value.value(),
            Self::Litres(value) => value.value(),
        }
    }

    /// Converts this volume to millilitres using exact metric unit factors.
    ///
    /// Arithmetic uses at most 28 fractional decimal places and the `Decimal` 96-bit
    /// coefficient. When precision must be reduced, midpoint ties round to nearest even.
    ///
    /// # Errors
    ///
    /// Returns [`QuantityError::Arithmetic`] if the result is outside the representable
    /// decimal range.
    pub fn to_millilitres(self) -> Result<Millilitres, QuantityError> {
        match self {
            Self::Millilitres(value) => Ok(value),
            Self::Litres(value) => {
                Millilitres::try_new(checked_mul(value.value(), Decimal::from(1000))?)
            }
        }
    }
}

impl sealed::Sealed for Volume {}

impl Measurement for Volume {
    fn value(self) -> Decimal {
        Volume::value(self)
    }

    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
        match unit {
            "ml" => Ok(Self::Millilitres(Millilitres::parse_with_unit(
                value, unit,
            )?)),
            "l" => Ok(Self::Litres(Litres::parse_with_unit(value, unit)?)),
            _ => Err(QuantityError::Unit(unit.to_owned())),
        }
    }

    fn checked_scale(self, factor: Decimal) -> Result<Self, QuantityError> {
        match self {
            Self::Millilitres(value) => Ok(Self::Millilitres(value.checked_scale(factor)?)),
            Self::Litres(value) => Ok(Self::Litres(value.checked_scale(factor)?)),
        }
    }
}

impl FromStr for Volume {
    type Err = QuantityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (number, unit) = split_quantity(value)?;
        <Self as Measurement>::parse_with_unit(number, unit)
    }
}

impl fmt::Display for Volume {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Millilitres(value) => value.fmt(f),
            Self::Litres(value) => value.fmt(f),
        }
    }
}

/// Applies fixed-point multiplication and maps representational overflow to a domain error.
fn checked_mul(left: Decimal, right: Decimal) -> Result<Decimal, QuantityError> {
    left.checked_mul(right).ok_or(QuantityError::Arithmetic)
}

/// Applies fixed-point division and maps representational overflow to a domain error.
fn checked_div(left: Decimal, right: Decimal) -> Result<Decimal, QuantityError> {
    left.checked_div(right).ok_or(QuantityError::Arithmetic)
}

impl fmt::Display for MeasuredQuantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mass(value) => value.fmt(f),
            Self::Volume(value) => value.fmt(f),
        }
    }
}

impl Serialize for MeasuredQuantity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for MeasuredQuantity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

mod sealed {
    pub trait Sealed {}
}

fn qualifier_and_amount(value: &str) -> (Qualifier, &str) {
    [
        ("<=", Qualifier::AtMost),
        (">=", Qualifier::AtLeast),
        ("≤", Qualifier::AtMost),
        ("≥", Qualifier::AtLeast),
        ("<", Qualifier::LessThan),
        (">", Qualifier::GreaterThan),
        ("~", Qualifier::Approximate),
    ]
    .into_iter()
    .find_map(|(prefix, qualifier)| value.strip_prefix(prefix).map(|rest| (qualifier, rest)))
    .unwrap_or((Qualifier::Exact, value))
}

fn split_quantity(value: &str) -> Result<(&str, &str), QuantityError> {
    value
        .trim()
        .rsplit_once(char::is_whitespace)
        .map(|(number, unit)| (number.trim(), unit))
        .ok_or_else(|| QuantityError::Unit(value.to_owned()))
}

fn parse_decimal(value: &str) -> Result<Decimal, QuantityError> {
    let expanded;
    let value = if let Some((base, exponent)) = value.split_once(['e', 'E']) {
        expanded = expand_decimal(base, exponent)
            .ok_or_else(|| QuantityError::Decimal(value.to_owned()))?;
        &expanded
    } else {
        value
    };
    // from_str_exact rejects excess scale instead of rounding to the decimal capacity.
    Decimal::from_str_exact(value).map_err(|_| QuantityError::Decimal(value.to_owned()))
}

fn expand_decimal(base: &str, exponent: &str) -> Option<String> {
    let exponent: i32 = exponent.parse().ok()?;
    // Bound allocation before expanding an untrusted exponent. A fixed-point
    // value has at most 29 integer and 28 fractional digits.
    if !(-56..=56).contains(&exponent) || base.len() > 86 {
        return None;
    }
    let (sign, base) = match base.strip_prefix('-') {
        Some(base) => ("-", base),
        None => ("", base.strip_prefix('+').unwrap_or(base)),
    };
    let (integer, fraction) = base.split_once('.').unwrap_or((base, ""));
    if integer.is_empty()
        || !integer
            .bytes()
            .chain(fraction.bytes())
            .all(|digit| digit.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{integer}{fraction}");
    let point = i32::try_from(integer.len()).ok()?.checked_add(exponent)?;
    if point <= 0 {
        let zeros = usize::try_from(point.checked_neg()?).ok()?;
        Some(format!("{sign}0.{}{digits}", "0".repeat(zeros)))
    } else {
        let point = usize::try_from(point).ok()?;
        if point >= digits.len() {
            Some(format!(
                "{sign}{digits}{}",
                "0".repeat(point - digits.len())
            ))
        } else {
            Some(format!(
                "{sign}{}.{}",
                digits.get(..point)?,
                digits.get(point..)?
            ))
        }
    }
}

#[cfg(test)]
#[path = "../../unit-tests/nutrition_model/quantity.rs"]
mod tests;
