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
    #[error("serving quantities must be positive")]
    NonPositive,
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

/// A measured nutrient amount with its source-supplied qualification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported<T> {
    pub qualifier: Qualifier,
    pub amount: T,
}

impl<T: FromStr<Err = QuantityError>> FromStr for Reported<T> {
    type Err = QuantityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        let (qualifier, rest) = [
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
        .unwrap_or((Qualifier::Exact, value));
        Ok(Self {
            qualifier,
            amount: rest.trim().parse()?,
        })
    }
}

impl<T: fmt::Display> fmt::Display for Reported<T> {
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

impl<T: fmt::Display> Serialize for Reported<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de, T: FromStr<Err = QuantityError>> Deserialize<'de> for Reported<T> {
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
            /// Returns the exact fixed-point value in this unit.
            pub const fn value(self) -> Decimal {
                self.0
            }
        }

        impl FromStr for $name {
            type Err = QuantityError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let (number, unit) = split_quantity(value)?;
                if unit != $unit {
                    return Err(QuantityError::Unit(unit.to_owned()));
                }
                Ok(Self(parse_decimal(number)?))
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
    pub(crate) fn is_zero(self) -> bool {
        match self {
            Self::Grams(value) => value.value().is_zero(),
            Self::Milligrams(value) => value.value().is_zero(),
            Self::Micrograms(value) => value.value().is_zero(),
        }
    }
}

impl FromStr for Mass {
    type Err = QuantityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match split_quantity(value)?.1 {
            "g" => Ok(Self::Grams(value.parse()?)),
            "mg" => Ok(Self::Milligrams(value.parse()?)),
            "µg" => Ok(Self::Micrograms(value.parse()?)),
            unit => Err(QuantityError::Unit(unit.to_owned())),
        }
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
    pub(crate) fn is_zero(self) -> bool {
        match self {
            Self::Kilocalories(value) => value.value().is_zero(),
            Self::Kilojoules(value) => value.value().is_zero(),
        }
    }
}

impl FromStr for Energy {
    type Err = QuantityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match split_quantity(value)?.1 {
            "kcal" => Ok(Self::Kilocalories(value.parse()?)),
            "kJ" => Ok(Self::Kilojoules(value.parse()?)),
            unit => Err(QuantityError::Unit(unit.to_owned())),
        }
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

/// A positive measured serving quantity. Mass and volume remain distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServingQuantity(MeasuredQuantity);

impl ServingQuantity {
    /// Returns the measured mass, when the serving has a mass measurement.
    pub const fn mass(self) -> Option<Mass> {
        match self.0 {
            MeasuredQuantity::Mass(mass) => Some(mass),
            MeasuredQuantity::Volume(_) => None,
        }
    }

    /// Returns the measured volume, when the serving has a volume measurement.
    pub const fn volume(self) -> Option<Millilitres> {
        match self.0 {
            MeasuredQuantity::Volume(volume) => Some(volume),
            MeasuredQuantity::Mass(_) => None,
        }
    }
}

impl FromStr for ServingQuantity {
    type Err = QuantityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let quantity = if split_quantity(value)?.1 == "ml" {
            let volume: Millilitres = value.parse()?;
            if volume.value().is_zero() {
                return Err(QuantityError::NonPositive);
            }
            MeasuredQuantity::Volume(volume)
        } else {
            let mass: Mass = value.parse()?;
            if mass.is_zero() {
                return Err(QuantityError::NonPositive);
            }
            MeasuredQuantity::Mass(mass)
        };
        Ok(Self(quantity))
    }
}

impl fmt::Display for ServingQuantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            MeasuredQuantity::Mass(value) => value.fmt(f),
            MeasuredQuantity::Volume(value) => value.fmt(f),
        }
    }
}

impl Serialize for ServingQuantity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ServingQuantity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeasuredQuantity {
    Mass(Mass),
    Volume(Millilitres),
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
    let decimal =
        Decimal::from_str_exact(value).map_err(|_| QuantityError::Decimal(value.to_owned()))?;
    if decimal.is_sign_negative() && !decimal.is_zero() {
        return Err(QuantityError::Negative);
    }
    Ok(decimal)
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
