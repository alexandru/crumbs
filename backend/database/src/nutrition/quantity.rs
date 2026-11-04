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

/// A nonnegative decimal measurement that can parse a value in a supported unit.
pub trait Measurement: sealed::Sealed + Copy + fmt::Display + FromStr<Err = QuantityError> {
    /// Returns whether this quantity is zero.
    fn is_zero(self) -> bool;

    /// Parses a decimal value and its unit without first formatting them together.
    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError>;
}

/// A measured nutrient amount with its source-supplied qualification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported<T: Measurement> {
    qualifier: Qualifier,
    amount: T,
}

impl<T: Measurement> Reported<T> {
    /// Creates a reported amount, rejecting an impossible strict upper bound.
    pub fn new(qualifier: Qualifier, amount: T) -> Result<Self, QuantityError> {
        if qualifier == Qualifier::LessThan && amount.is_zero() {
            return Err(QuantityError::ZeroStrictUpperBound);
        }
        Ok(Self { qualifier, amount })
    }

    /// Creates a strict upper bound from an already validated positive limit.
    pub fn less_than(limit: Positive<T>) -> Self {
        Self {
            qualifier: Qualifier::LessThan,
            amount: limit.get(),
        }
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
    pub const fn amount(&self) -> &T {
        &self.amount
    }

    /// Returns the measured amount by value.
    pub fn into_amount(self) -> T {
        self.amount
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
            fn is_zero(self) -> bool {
                self.0.is_zero()
            }

            fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
                if unit != $unit {
                    return Err(QuantityError::Unit(unit.to_owned()));
                }
                Self::try_new(parse_decimal(value)?)
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
measurement!(Kilocalories, "kcal");
measurement!(Kilojoules, "kJ");

/// A mass amount retaining the supplied supported unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mass {
    Grams(Grams),
    Milligrams(Milligrams),
    Micrograms(Micrograms),
}

impl Mass {
    /// Returns whether this measured amount is zero.
    pub fn is_zero(self) -> bool {
        match self {
            Self::Grams(value) => value.value().is_zero(),
            Self::Milligrams(value) => value.value().is_zero(),
            Self::Micrograms(value) => value.value().is_zero(),
        }
    }
}

impl sealed::Sealed for Mass {}

impl Measurement for Mass {
    fn is_zero(self) -> bool {
        Mass::is_zero(self)
    }

    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
        match unit {
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
    /// Returns whether this measured amount is zero.
    pub fn is_zero(self) -> bool {
        match self {
            Self::Kilocalories(value) => value.value().is_zero(),
            Self::Kilojoules(value) => value.value().is_zero(),
        }
    }
}

impl sealed::Sealed for Energy {}

impl Measurement for Energy {
    fn is_zero(self) -> bool {
        Energy::is_zero(self)
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

/// A positive measurement of any supported dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Positive<T: Measurement>(T);

impl<T: Measurement> Positive<T> {
    /// Creates a positive measurement.
    pub fn try_new(value: T) -> Result<Self, QuantityError> {
        if value.is_zero() {
            return Err(QuantityError::NonPositive);
        }
        Ok(Self(value))
    }

    /// Returns the positive measurement.
    pub const fn get(self) -> T {
        self.0
    }
}

/// A positive serving quantity with its mass or volume dimension preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasuredQuantity {
    Mass(Positive<Mass>),
    Volume(Positive<Millilitres>),
}

impl MeasuredQuantity {
    /// Returns the measured mass, when the serving has a mass measurement.
    pub const fn mass(self) -> Option<Mass> {
        match self {
            Self::Mass(mass) => Some(mass.get()),
            Self::Volume(_) => None,
        }
    }

    /// Returns the measured volume, when the serving has a volume measurement.
    pub const fn volume(self) -> Option<Millilitres> {
        match self {
            Self::Volume(volume) => Some(volume.get()),
            Self::Mass(_) => None,
        }
    }
}

impl FromStr for MeasuredQuantity {
    type Err = QuantityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (_, unit) = split_quantity(value)?;
        let quantity = if unit == "ml" {
            let volume: Millilitres = value.parse()?;
            Self::Volume(Positive::try_new(volume)?)
        } else {
            let mass: Mass = value.parse()?;
            Self::Mass(Positive::try_new(mass)?)
        };
        Ok(quantity)
    }
}

impl fmt::Display for MeasuredQuantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mass(value) => value.get().fmt(f),
            Self::Volume(value) => value.get().fmt(f),
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
