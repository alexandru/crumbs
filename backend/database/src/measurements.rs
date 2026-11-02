//! Exact, unit-bearing quantities used by nutrition data.
//!
//! Decimal values use a 96-bit coefficient and a base-10 scale from 0 through 28.
//! Physical mass, energy, and volume quantities are nonnegative; servings are positive.
//! [`SignedMass`] preserves signed source-reported mass estimates.
//! Parsing rejects values outside this exact representation instead of rounding.

use std::fmt;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Errors produced while parsing or constructing a measurement.
#[derive(Debug, thiserror::Error)]
pub enum MeasurementError {
    /// The numeric text does not follow the supported decimal grammar.
    #[error("invalid decimal syntax: {input}")]
    InvalidDecimalSyntax {
        /// Decimal text that failed syntax validation.
        input: String,
    },
    /// The decimal cannot fit within the supported exact range or precision.
    #[error("decimal is outside the supported exact range or precision: {input}")]
    DecimalRange {
        /// Decimal text that exceeded the exact range or precision.
        input: String,
    },
    /// The decimal parser could not represent the expanded decimal exactly.
    #[error("decimal cannot be represented exactly: {input}: {source}")]
    DecimalRepresentation {
        /// Decimal text that could not be represented exactly.
        input: String,
        #[source]
        /// Error returned by the decimal parser.
        source: rust_decimal::Error,
    },
    /// A nutrition quantity is less than zero.
    #[error("measurement quantity cannot be negative")]
    NegativeQuantity,
    /// A serving quantity is zero or less.
    #[error("serving quantity must be greater than zero")]
    NonPositiveServing,
    /// The unit is not supported by this measurement boundary.
    #[error("unsupported measurement unit: {unit}")]
    UnsupportedUnit {
        /// Unit text that is not supported.
        unit: String,
    },
    /// The unit is supported, but does not match the required dimension or type.
    #[error("unit {unit} is incompatible with {expected}")]
    IncompatibleUnit {
        /// Unit text that does not match the expected measurement.
        unit: String,
        /// Required measurement type or dimension.
        expected: &'static str,
    },
}

/// A nonnegative mass in grams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grams(Decimal);

/// A nonnegative mass in milligrams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Milligrams(Decimal);

/// A nonnegative mass in micrograms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Micrograms(Decimal);

/// A nonnegative amount of energy in kilocalories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kilocalories(Decimal);

/// A nonnegative amount of energy in kilojoules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kilojoules(Decimal);

/// A nonnegative volume in millilitres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Millilitres(Decimal);

/// A nonnegative volume in litres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Litres(Decimal);

/// A mass with its supplied unit preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mass {
    /// A mass measured in grams.
    Grams(Grams),
    /// A mass measured in milligrams.
    Milligrams(Milligrams),
    /// A mass measured in micrograms.
    Micrograms(Micrograms),
}

/// A signed source-reported mass estimate, such as calculated carbohydrates.
///
/// Use this type for signed estimates supplied by a source. Physical measured
/// quantities and serving masses remain nonnegative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedMass {
    magnitude: Mass,
    negative: bool,
}

impl SignedMass {
    /// Returns the nonnegative mass magnitude and its supplied unit.
    pub fn magnitude(self) -> Mass {
        self.magnitude
    }

    /// Returns whether this value is negative.
    pub fn is_negative(self) -> bool {
        self.negative
    }
}

impl From<Mass> for SignedMass {
    fn from(magnitude: Mass) -> Self {
        Self {
            magnitude,
            negative: false,
        }
    }
}

impl FromStr for SignedMass {
    type Err = MeasurementError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let (negative, magnitude_input) = if let Some(magnitude_input) = input.strip_prefix('-') {
            let next = magnitude_input.trim_start();
            if next.starts_with('-') || next.starts_with('+') {
                return Err(MeasurementError::InvalidDecimalSyntax {
                    input: input.to_owned(),
                });
            }
            (true, magnitude_input)
        } else {
            (false, input)
        };

        let magnitude = magnitude_input.parse::<Mass>()?;
        Ok(Self {
            magnitude,
            negative: negative && !mass_is_zero(magnitude),
        })
    }
}

impl fmt::Display for SignedMass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negative {
            formatter.write_str("-")?;
        }
        fmt::Display::fmt(&self.magnitude, formatter)
    }
}

impl Serialize for SignedMass {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SignedMass {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = String::deserialize(deserializer)?;
        input.parse().map_err(de::Error::custom)
    }
}

/// An energy quantity with its supplied unit preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Energy {
    /// An energy quantity measured in kilocalories.
    Kilocalories(Kilocalories),
    /// An energy quantity measured in kilojoules.
    Kilojoules(Kilojoules),
}

impl FromStr for Mass {
    type Err = MeasurementError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (number, unit) = split_quantity(input);
        let value = parse_decimal(number)?;
        match unit {
            "g" => Ok(Self::Grams(Grams::new(value)?)),
            "mg" => Ok(Self::Milligrams(Milligrams::new(value)?)),
            "µg" | "μg" | "ug" => Ok(Self::Micrograms(Micrograms::new(value)?)),
            other => Err(unit_error(other, "mass")),
        }
    }
}

impl fmt::Display for Mass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grams(value) => fmt::Display::fmt(value, formatter),
            Self::Milligrams(value) => fmt::Display::fmt(value, formatter),
            Self::Micrograms(value) => fmt::Display::fmt(value, formatter),
        }
    }
}

impl Serialize for Mass {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Mass {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = String::deserialize(deserializer)?;
        input.parse().map_err(de::Error::custom)
    }
}

impl FromStr for Energy {
    type Err = MeasurementError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (number, unit) = split_quantity(input);
        let value = parse_decimal(number)?;
        match unit {
            "kcal" => Ok(Self::Kilocalories(Kilocalories::new(value)?)),
            "kJ" => Ok(Self::Kilojoules(Kilojoules::new(value)?)),
            other => Err(unit_error(other, "energy")),
        }
    }
}

impl fmt::Display for Energy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kilocalories(value) => fmt::Display::fmt(value, formatter),
            Self::Kilojoules(value) => fmt::Display::fmt(value, formatter),
        }
    }
}

impl Serialize for Energy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Energy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = String::deserialize(deserializer)?;
        input.parse().map_err(de::Error::custom)
    }
}

/// The relation reported for an amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountQualifier {
    /// The supplied amount has no qualifier.
    Exact,
    /// The reported amount is below the stated value.
    LessThan,
    /// The reported amount is at most the stated value.
    LessThanOrEqual,
    /// The reported amount is above the stated value.
    GreaterThan,
    /// The reported amount is at least the stated value.
    GreaterThanOrEqual,
    /// The reported amount is approximate.
    Approximate,
}

/// A typed quantity together with the source's qualifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported<T> {
    /// The parsed amount.
    pub amount: T,
    /// The source's relation to the amount.
    pub qualifier: AmountQualifier,
}

impl<T> Reported<T> {
    /// Creates a reported amount from a typed quantity and qualifier.
    pub fn new(amount: T, qualifier: AmountQualifier) -> Self {
        Self { amount, qualifier }
    }
}

impl<T> FromStr for Reported<T>
where
    T: FromStr<Err = MeasurementError>,
{
    type Err = MeasurementError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let (qualifier, quantity) = if let Some(quantity) = input.strip_prefix("<=") {
            (AmountQualifier::LessThanOrEqual, quantity)
        } else if let Some(quantity) = input.strip_prefix("≥") {
            (AmountQualifier::GreaterThanOrEqual, quantity)
        } else if let Some(quantity) = input.strip_prefix("≤") {
            (AmountQualifier::LessThanOrEqual, quantity)
        } else if let Some(quantity) = input.strip_prefix(">=") {
            (AmountQualifier::GreaterThanOrEqual, quantity)
        } else if let Some(quantity) = input.strip_prefix('<') {
            (AmountQualifier::LessThan, quantity)
        } else if let Some(quantity) = input.strip_prefix('>') {
            (AmountQualifier::GreaterThan, quantity)
        } else if let Some(quantity) = input.strip_prefix('~') {
            (AmountQualifier::Approximate, quantity)
        } else {
            (AmountQualifier::Exact, input)
        };

        Ok(Self {
            amount: quantity.trim().parse()?,
            qualifier,
        })
    }
}

impl<T: fmt::Display> fmt::Display for Reported<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.qualifier {
            AmountQualifier::Exact => "",
            AmountQualifier::LessThan => "< ",
            AmountQualifier::LessThanOrEqual => "<= ",
            AmountQualifier::GreaterThan => "> ",
            AmountQualifier::GreaterThanOrEqual => ">= ",
            AmountQualifier::Approximate => "~ ",
        };
        write!(formatter, "{prefix}{}", self.amount)
    }
}

impl<T: fmt::Display> Serialize for Reported<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de, T> Deserialize<'de> for Reported<T>
where
    T: FromStr<Err = MeasurementError>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = String::deserialize(deserializer)?;
        input.parse().map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServingMeasure {
    /// A positive mass serving in its supplied unit.
    Mass(Mass),
    /// A positive volume serving measured in millilitres.
    Millilitres(Millilitres),
    /// A positive volume serving measured in litres.
    Litres(Litres),
}

/// A positive serving quantity that preserves mass or volume units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServingQuantity(ServingMeasure);

impl ServingQuantity {
    /// Returns the typed mass or volume carried by this serving.
    pub fn measure(self) -> ServingMeasure {
        self.0
    }
}

impl FromStr for ServingQuantity {
    type Err = MeasurementError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (number, unit) = split_quantity(input);
        let value = parse_decimal(number)?;
        ensure_nonnegative(value)?;
        if value.is_zero() {
            return Err(MeasurementError::NonPositiveServing);
        }

        let measure = match unit {
            "g" => ServingMeasure::Mass(Mass::Grams(Grams::new(value)?)),
            "mg" => ServingMeasure::Mass(Mass::Milligrams(Milligrams::new(value)?)),
            "µg" | "μg" | "ug" => ServingMeasure::Mass(Mass::Micrograms(Micrograms::new(value)?)),
            "ml" => ServingMeasure::Millilitres(Millilitres::new(value)?),
            "l" => ServingMeasure::Litres(Litres::new(value)?),
            other => return Err(unit_error(other, "serving mass or volume")),
        };
        Ok(Self(measure))
    }
}

impl fmt::Display for ServingQuantity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ServingMeasure::Mass(value) => fmt::Display::fmt(&value, formatter),
            ServingMeasure::Millilitres(value) => fmt::Display::fmt(&value, formatter),
            ServingMeasure::Litres(value) => fmt::Display::fmt(&value, formatter),
        }
    }
}

impl Serialize for ServingQuantity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ServingQuantity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = String::deserialize(deserializer)?;
        input.parse().map_err(de::Error::custom)
    }
}

macro_rules! impl_measurement {
    ($type:ty, $unit:literal, $aliases:expr, $expected:literal) => {
        impl $type {
            /// Creates a quantity after checking that it is nonnegative.
            ///
            /// # Errors
            ///
            /// Returns [`MeasurementError::NegativeQuantity`] when `value` is less than zero.
            pub fn new(value: Decimal) -> Result<Self, MeasurementError> {
                ensure_nonnegative(value)?;
                Ok(Self(value))
            }

            /// Returns the exact decimal quantity in this type's unit.
            pub fn value(self) -> Decimal {
                self.0
            }
        }

        impl FromStr for $type {
            type Err = MeasurementError;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                let value = parse_unit_value(input, $aliases, $expected)?;
                Self::new(value)
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{} {}", self.0, $unit)
            }
        }

        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let input = String::deserialize(deserializer)?;
                input.parse().map_err(de::Error::custom)
            }
        }
    };
}

impl_measurement!(Grams, "g", &["g"], "grams");
impl_measurement!(Milligrams, "mg", &["mg"], "milligrams");
impl_measurement!(Micrograms, "µg", &["µg", "μg", "ug"], "micrograms");
impl_measurement!(Kilocalories, "kcal", &["kcal"], "kilocalories");
impl_measurement!(Kilojoules, "kJ", &["kJ"], "kilojoules");
impl_measurement!(Millilitres, "ml", &["ml"], "millilitres");
impl_measurement!(Litres, "l", &["l"], "litres");

/// Parses decimal and scientific notation without binary floating point.
pub(crate) fn parse_decimal(input: &str) -> Result<Decimal, MeasurementError> {
    let lexeme = decimal_lexeme(input).ok_or_else(|| MeasurementError::InvalidDecimalSyntax {
        input: input.to_owned(),
    })?;

    let mantissa_start = if lexeme.has_sign { 1 } else { 0 };
    let mantissa_end = lexeme.exponent_marker.unwrap_or(input.len());
    let mantissa = input.get(mantissa_start..mantissa_end).ok_or_else(|| {
        MeasurementError::InvalidDecimalSyntax {
            input: input.to_owned(),
        }
    })?;
    let mut coefficient = String::with_capacity(mantissa.len());
    for byte in mantissa.bytes().filter(|byte| *byte != b'.') {
        coefficient.push(char::from(byte));
    }

    let first_nonzero = coefficient.bytes().position(|digit| digit != b'0');
    let is_zero = match first_nonzero {
        Some(index) => {
            coefficient = coefficient
                .get(index..)
                .ok_or_else(|| MeasurementError::InvalidDecimalSyntax {
                    input: input.to_owned(),
                })?
                .to_owned();
            false
        }
        None => {
            coefficient = "0".to_owned();
            true
        }
    };

    let exponent = match lexeme.exponent_marker {
        Some(marker) => match parse_exponent(input, marker) {
            Some(exponent) => exponent,
            None if is_zero => return Ok(Decimal::ZERO),
            None => {
                return Err(MeasurementError::DecimalRange {
                    input: input.to_owned(),
                });
            }
        },
        None => 0,
    };
    let fraction_digits = match i64::try_from(lexeme.fraction_digits) {
        Ok(fraction_digits) => fraction_digits,
        Err(_) if is_zero => return Ok(Decimal::ZERO),
        Err(_) => {
            return Err(MeasurementError::DecimalRange {
                input: input.to_owned(),
            });
        }
    };
    let mut scale = match fraction_digits.checked_sub(exponent) {
        Some(scale) => scale,
        None if is_zero => return Ok(Decimal::ZERO),
        None => {
            return Err(MeasurementError::DecimalRange {
                input: input.to_owned(),
            });
        }
    };

    if is_zero {
        if !(0..=MAX_DECIMAL_SCALE).contains(&scale) {
            return Ok(Decimal::ZERO);
        }
        let scale = usize::try_from(scale).map_err(|_| MeasurementError::DecimalRange {
            input: input.to_owned(),
        })?;
        let expanded = decimal_string(false, "0", scale, input)?;
        return Decimal::from_str_exact(&expanded).map_err(|source| {
            MeasurementError::DecimalRepresentation {
                input: input.to_owned(),
                source,
            }
        });
    }

    if scale < 0 {
        let zeros_to_append = scale
            .checked_neg()
            .and_then(|value| usize::try_from(value).ok());
        let Some(zeros_to_append) = zeros_to_append else {
            return Err(MeasurementError::DecimalRange {
                input: input.to_owned(),
            });
        };
        if zeros_to_append > MAX_COEFFICIENT_DIGITS
            || coefficient
                .len()
                .checked_add(zeros_to_append)
                .is_none_or(|length| length > MAX_COEFFICIENT_DIGITS)
        {
            return Err(MeasurementError::DecimalRange {
                input: input.to_owned(),
            });
        }
        for _ in 0..zeros_to_append {
            coefficient.push('0');
        }
        scale = 0;
    }

    while scale > MAX_DECIMAL_SCALE || (scale > 0 && coefficient_overflows(&coefficient)) {
        if scale <= 0 || !coefficient.ends_with('0') {
            return Err(MeasurementError::DecimalRange {
                input: input.to_owned(),
            });
        }
        coefficient.pop();
        scale = scale
            .checked_sub(1)
            .ok_or_else(|| MeasurementError::DecimalRange {
                input: input.to_owned(),
            })?;
    }

    if coefficient_overflows(&coefficient) || scale > MAX_DECIMAL_SCALE {
        return Err(MeasurementError::DecimalRange {
            input: input.to_owned(),
        });
    }

    let scale = usize::try_from(scale).map_err(|_| MeasurementError::DecimalRange {
        input: input.to_owned(),
    })?;
    let expanded = decimal_string(lexeme.negative, &coefficient, scale, input)?;
    Decimal::from_str_exact(&expanded).map_err(|source| MeasurementError::DecimalRepresentation {
        input: input.to_owned(),
        source,
    })
}

const MAX_DECIMAL_SCALE: i64 = 28;
const MAX_COEFFICIENT_DIGITS: usize = 29;
const MAX_COEFFICIENT: &str = "79228162514264337593543950335";

#[derive(Clone, Copy)]
struct DecimalLexeme {
    negative: bool,
    has_sign: bool,
    fraction_digits: usize,
    exponent_marker: Option<usize>,
}

fn decimal_lexeme(input: &str) -> Option<DecimalLexeme> {
    let bytes = input.as_bytes();
    let mut index: usize = 0;
    let has_sign = matches!(bytes.first(), Some(b'+') | Some(b'-'));
    let negative = bytes.first() == Some(&b'-');
    if has_sign {
        index = index.saturating_add(1);
    }

    let integer_start = index;
    while matches!(bytes.get(index), Some(b'0'..=b'9')) {
        index = index.saturating_add(1);
    }
    let integer_digits = index.saturating_sub(integer_start);

    let mut fraction_digits = 0;
    if bytes.get(index) == Some(&b'.') {
        index = index.saturating_add(1);
        let fraction_start = index;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index = index.saturating_add(1);
        }
        fraction_digits = index.saturating_sub(fraction_start);
    }
    if integer_digits == 0 && fraction_digits == 0 {
        return None;
    }

    let exponent_marker = if matches!(bytes.get(index), Some(b'e') | Some(b'E')) {
        let marker = index;
        index = index.saturating_add(1);
        if matches!(bytes.get(index), Some(b'+') | Some(b'-')) {
            index = index.saturating_add(1);
        }
        let exponent_start = index;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index = index.saturating_add(1);
        }
        if index == exponent_start {
            return None;
        }
        Some(marker)
    } else {
        None
    };

    (index == bytes.len()).then_some(DecimalLexeme {
        negative,
        has_sign,
        fraction_digits,
        exponent_marker,
    })
}

fn parse_exponent(input: &str, marker: usize) -> Option<i64> {
    let bytes = input.as_bytes();
    let mut index = marker.saturating_add(1);
    let negative = bytes.get(index) == Some(&b'-');
    if matches!(bytes.get(index), Some(b'+') | Some(b'-')) {
        index = index.saturating_add(1);
    }

    let mut exponent = 0_i64;
    while let Some(byte @ b'0'..=b'9') = bytes.get(index).copied() {
        exponent = exponent
            .checked_mul(10)?
            .checked_add(i64::from(byte - b'0'))?;
        index = index.saturating_add(1);
    }
    if negative {
        exponent.checked_neg()
    } else {
        Some(exponent)
    }
}

fn coefficient_overflows(coefficient: &str) -> bool {
    coefficient.len() > MAX_COEFFICIENT_DIGITS
        || (coefficient.len() == MAX_COEFFICIENT_DIGITS && coefficient > MAX_COEFFICIENT)
}

fn decimal_string(
    negative: bool,
    coefficient: &str,
    scale: usize,
    input: &str,
) -> Result<String, MeasurementError> {
    let mut expanded = String::with_capacity(60);
    if negative {
        expanded.push('-');
    }

    if scale == 0 {
        expanded.push_str(coefficient);
    } else if coefficient.len() > scale {
        let decimal_point = coefficient.len() - scale;
        let integer =
            coefficient
                .get(..decimal_point)
                .ok_or_else(|| MeasurementError::DecimalRange {
                    input: input.to_owned(),
                })?;
        let fractional =
            coefficient
                .get(decimal_point..)
                .ok_or_else(|| MeasurementError::DecimalRange {
                    input: input.to_owned(),
                })?;
        expanded.push_str(integer);
        expanded.push('.');
        expanded.push_str(fractional);
    } else {
        expanded.push_str("0.");
        for _ in 0..(scale - coefficient.len()) {
            expanded.push('0');
        }
        expanded.push_str(coefficient);
    }
    Ok(expanded)
}

fn ensure_nonnegative(value: Decimal) -> Result<(), MeasurementError> {
    if value < Decimal::ZERO {
        Err(MeasurementError::NegativeQuantity)
    } else {
        Ok(())
    }
}

fn mass_is_zero(mass: Mass) -> bool {
    match mass {
        Mass::Grams(value) => value.value().is_zero(),
        Mass::Milligrams(value) => value.value().is_zero(),
        Mass::Micrograms(value) => value.value().is_zero(),
    }
}

fn parse_unit_value(
    input: &str,
    accepted_units: &[&str],
    expected: &'static str,
) -> Result<Decimal, MeasurementError> {
    let (number, unit) = split_quantity(input);
    if accepted_units.contains(&unit) {
        parse_decimal(number)
    } else {
        Err(unit_error(unit, expected))
    }
}

fn split_quantity(input: &str) -> (&str, &str) {
    let input = input.trim();
    let split = input
        .char_indices()
        .find_map(|(index, character)| {
            (!matches!(character, '0'..='9' | '+' | '-' | '.' | 'e' | 'E')).then_some(index)
        })
        .unwrap_or(input.len());
    let (number, unit) = input.split_at(split);
    (number, unit.trim_start())
}

fn unit_error(unit: &str, expected: &'static str) -> MeasurementError {
    if is_supported_unit(unit) {
        MeasurementError::IncompatibleUnit {
            unit: unit.to_owned(),
            expected,
        }
    } else {
        MeasurementError::UnsupportedUnit {
            unit: unit.to_owned(),
        }
    }
}

fn is_supported_unit(unit: &str) -> bool {
    matches!(
        unit,
        "g" | "mg" | "µg" | "μg" | "ug" | "kcal" | "kJ" | "ml" | "l"
    )
}

#[cfg(test)]
#[path = "../unit-tests/measurements.rs"]
mod tests;
