//! Unit-bearing measurement quantities for nutrition data.
//!
//! Quantities serialize as unit-bearing strings such as `"15 mg"`, `"0.55 µg"`,
//! `"< 0.5 g"`, and `"~ 15 mg"`, and parse those strings directly back into
//! concrete unit types. Amounts are fixed-point decimals: parsing and
//! serialization never pass through binary floating point, and amounts that do
//! not fit the decimal representation are rejected with a typed error instead
//! of being rounded.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;
use serde::de::Visitor;
use serde_json::Number as JsonNumber;

pub use rust_decimal::Decimal;

/// A comparison or approximation stated for a reported amount.
///
/// An amount without a qualifier is an exact value. Qualifiers keep
/// source-stated comparisons and approximations from collapsing into
/// unqualified numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Qualifier {
    /// Less than the amount, written `<`.
    LessThan,
    /// Greater than the amount, written `>`.
    GreaterThan,
    /// At most the amount, written `<=` or `≤`.
    AtMost,
    /// At least the amount, written `>=` or `≥`.
    AtLeast,
    /// Approximately the amount, written `~`.
    Approximately,
}

impl Qualifier {
    fn parse_token(token: &str) -> Option<Self> {
        match token {
            "<" => Some(Self::LessThan),
            ">" => Some(Self::GreaterThan),
            "<=" | "≤" => Some(Self::AtMost),
            ">=" | "≥" => Some(Self::AtLeast),
            "~" => Some(Self::Approximately),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::LessThan => "<",
            Self::GreaterThan => ">",
            Self::AtMost => "<=",
            Self::AtLeast => ">=",
            Self::Approximately => "~",
        }
    }
}

/// A mass unit for measured quantities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MassUnit {
    /// Grams, written `g`.
    Gram,
    /// Milligrams, written `mg`.
    Milligram,
    /// Micrograms, written `µg`.
    Microgram,
}

impl MassUnit {
    /// Returns the canonical unit token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gram => GRAM,
            Self::Milligram => MILLIGRAM,
            Self::Microgram => MICROGRAM,
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        match canonical_unit(token) {
            Some(GRAM) => Some(Self::Gram),
            Some(MILLIGRAM) => Some(Self::Milligram),
            Some(MICROGRAM) => Some(Self::Microgram),
            _ => None,
        }
    }
}

/// Defines a unit-specific reported amount type with a fixed unit token.
macro_rules! reported_amount {
    (
        $(#[$doc:meta])*
        $name:ident,
        $unit:ident
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name {
            value: Decimal,
            qualifier: Option<Qualifier>,
        }

        impl $name {
            /// Parses an amount from a unit-bearing string.
            ///
            /// # Errors
            ///
            /// Returns [`MeasureError`] when the string is malformed, the unit
            /// does not match this type, or the amount is not an exact
            /// decimal.
            pub fn parse(text: &str) -> Result<Self, MeasureError> {
                let (qualifier, value, unit) = split_reported(text)?;
                if canonical_unit(&unit) != Some($unit) {
                    return Err(MeasureError::UnsupportedUnit { unit });
                }
                Ok(Self { value, qualifier })
            }

            /// Builds an amount from an exact decimal and optional qualifier.
            pub fn new(value: Decimal, qualifier: Option<Qualifier>) -> Self {
                Self { value, qualifier }
            }

            /// Returns the exact reported amount.
            pub fn value(&self) -> Decimal {
                self.value
            }

            /// Returns the stated comparison or approximation, if any.
            pub fn qualifier(&self) -> Option<Qualifier> {
                self.qualifier
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.qualifier {
                    Some(qualifier) => {
                        qualifier.as_str().fmt(formatter)?;
                        formatter.write_str(" ")?;
                    }
                    None => {}
                }
                self.value.fmt(formatter)?;
                formatter.write_str(" ")?;
                formatter.write_str($unit)
            }
        }

        bearer_string!($name);
    };
}

/// Implements string-based serialization for a type with `parse` and `Display`.
macro_rules! bearer_string {
    ($name:ty) => {
        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_str(BearerVisitor(<$name>::parse))
            }
        }
    };
}

reported_amount!(
    /// A reported amount in grams, serialized as `"<qualifier> <amount> g"`.
    ///
    /// The amount is an exact fixed-point decimal kept as reported by the
    /// source, including zero and negative values; the unit is fixed to grams.
    Grams,
    GRAM
);

reported_amount!(
    /// A reported amount in milligrams, serialized as
    /// `"<qualifier> <amount> mg"`.
    Milligrams,
    MILLIGRAM
);

reported_amount!(
    /// A reported amount in micrograms, serialized as
    /// `"<qualifier> <amount> µg"`.
    Micrograms,
    MICROGRAM
);

reported_amount!(
    /// A reported amount in kilocalories, serialized as
    /// `"<qualifier> <amount> kcal"`.
    Kilocalories,
    KILOCALORIE
);

reported_amount!(
    /// A reported amount in kilojoules, serialized as
    /// `"<qualifier> <amount> kJ"`.
    Kilojoules,
    KILOJOULE
);

/// A reported mass amount in the unit supplied by its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mass {
    /// Grams.
    Grams(Grams),
    /// Milligrams.
    Milligrams(Milligrams),
    /// Micrograms.
    Micrograms(Micrograms),
}

impl Mass {
    /// Parses a mass amount such as `"150 mg"`, `"0.55 µg"`, or `"< 0.5 g"`.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError`] when the string is malformed, the unit is not a
    /// supported mass unit, or the amount is not an exact decimal.
    pub fn parse(text: &str) -> Result<Self, MeasureError> {
        let (qualifier, value, unit) = split_reported(text)?;
        Self::from_parts(&unit, value, qualifier)
    }

    /// Builds a mass amount from a unit name and exact decimal.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError::UnsupportedUnit`] when the unit is not a
    /// supported mass unit.
    pub fn from_parts(
        unit: &str,
        value: Decimal,
        qualifier: Option<Qualifier>,
    ) -> Result<Self, MeasureError> {
        match MassUnit::from_token(unit) {
            Some(MassUnit::Gram) => Ok(Self::Grams(Grams::new(value, qualifier))),
            Some(MassUnit::Milligram) => Ok(Self::Milligrams(Milligrams::new(value, qualifier))),
            Some(MassUnit::Microgram) => Ok(Self::Micrograms(Micrograms::new(value, qualifier))),
            None => Err(MeasureError::UnsupportedUnit {
                unit: unit.to_owned(),
            }),
        }
    }

    /// Returns the exact reported amount.
    pub fn value(&self) -> Decimal {
        match self {
            Self::Grams(amount) => amount.value(),
            Self::Milligrams(amount) => amount.value(),
            Self::Micrograms(amount) => amount.value(),
        }
    }

    /// Returns the stated comparison or approximation, if any.
    pub fn qualifier(&self) -> Option<Qualifier> {
        match self {
            Self::Grams(amount) => amount.qualifier(),
            Self::Milligrams(amount) => amount.qualifier(),
            Self::Micrograms(amount) => amount.qualifier(),
        }
    }
}

impl fmt::Display for Mass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grams(amount) => amount.fmt(formatter),
            Self::Milligrams(amount) => amount.fmt(formatter),
            Self::Micrograms(amount) => amount.fmt(formatter),
        }
    }
}

bearer_string!(Mass);

/// A reported energy amount in the unit supplied by its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Energy {
    /// Kilocalories.
    Kilocalories(Kilocalories),
    /// Kilojoules.
    Kilojoules(Kilojoules),
}

impl Energy {
    /// Parses an energy amount such as `"210 kcal"` or `"960 kJ"`.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError`] when the string is malformed, the unit is not a
    /// supported energy unit, or the amount is not an exact decimal.
    pub fn parse(text: &str) -> Result<Self, MeasureError> {
        let (qualifier, value, unit) = split_reported(text)?;
        Self::from_parts(&unit, value, qualifier)
    }

    /// Builds an energy amount from a unit name and exact decimal.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError::UnsupportedUnit`] when the unit is not a
    /// supported energy unit.
    pub fn from_parts(
        unit: &str,
        value: Decimal,
        qualifier: Option<Qualifier>,
    ) -> Result<Self, MeasureError> {
        match canonical_unit(unit) {
            Some(KILOCALORIE) => Ok(Self::Kilocalories(Kilocalories::new(value, qualifier))),
            Some(KILOJOULE) => Ok(Self::Kilojoules(Kilojoules::new(value, qualifier))),
            _ => Err(MeasureError::UnsupportedUnit {
                unit: unit.to_owned(),
            }),
        }
    }

    /// Returns the exact reported amount.
    pub fn value(&self) -> Decimal {
        match self {
            Self::Kilocalories(amount) => amount.value(),
            Self::Kilojoules(amount) => amount.value(),
        }
    }

    /// Returns the stated comparison or approximation, if any.
    pub fn qualifier(&self) -> Option<Qualifier> {
        match self {
            Self::Kilocalories(amount) => amount.qualifier(),
            Self::Kilojoules(amount) => amount.qualifier(),
        }
    }
}

impl fmt::Display for Energy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kilocalories(amount) => amount.fmt(formatter),
            Self::Kilojoules(amount) => amount.fmt(formatter),
        }
    }
}

bearer_string!(Energy);

/// An exact positive mass, such as a serving's gram weight.
///
/// Measured quantities carry no qualifier: they state the measured amount
/// itself, not a reported bound. Serving and reference quantities must be
/// positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeasuredMass {
    value: Decimal,
    unit: MassUnit,
}

impl MeasuredMass {
    /// Builds a measured mass, rejecting amounts that are not positive.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError::NotPositive`] for zero or negative amounts.
    pub fn new(unit: MassUnit, value: Decimal) -> Result<Self, MeasureError> {
        if value <= Decimal::ZERO {
            return Err(MeasureError::NotPositive { found: value });
        }
        Ok(Self { value, unit })
    }

    /// Parses a measured mass such as `"30 g"`.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError`] when the string is malformed or qualified, the
    /// unit is not a supported mass unit, or the amount is not positive.
    pub fn parse(text: &str) -> Result<Self, MeasureError> {
        let (qualifier, value, unit) = split_reported(text)?;
        reject_qualifier(qualifier, text)?;
        let unit = MassUnit::from_token(&unit).ok_or(MeasureError::UnsupportedUnit { unit })?;
        Self::new(unit, value)
    }

    /// Returns the exact measured amount.
    pub fn value(&self) -> Decimal {
        self.value
    }

    /// Returns the mass unit.
    pub fn unit(&self) -> MassUnit {
        self.unit
    }
}

impl fmt::Display for MeasuredMass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.value, self.unit.as_str())
    }
}

bearer_string!(MeasuredMass);

/// An exact positive volume in millilitres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeasuredVolume {
    value: Decimal,
}

impl MeasuredVolume {
    /// Builds a measured volume, rejecting amounts that are not positive.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError::NotPositive`] for zero or negative amounts.
    pub fn new(value: Decimal) -> Result<Self, MeasureError> {
        if value <= Decimal::ZERO {
            return Err(MeasureError::NotPositive { found: value });
        }
        Ok(Self { value })
    }

    /// Parses a measured volume such as `"250 ml"`.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError`] when the string is malformed or qualified, the
    /// unit is not millilitres, or the amount is not positive.
    pub fn parse(text: &str) -> Result<Self, MeasureError> {
        let (qualifier, value, unit) = split_reported(text)?;
        reject_qualifier(qualifier, text)?;
        match canonical_unit(&unit) {
            Some(MILLILITRE) => Self::new(value),
            _ => Err(MeasureError::UnsupportedUnit { unit }),
        }
    }

    /// Returns the exact measured amount in millilitres.
    pub fn value(&self) -> Decimal {
        self.value
    }
}

impl fmt::Display for MeasuredVolume {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.value, MILLILITRE)
    }
}

bearer_string!(MeasuredVolume);

/// An exact positive mass or volume, such as a serving's quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasuredQuantity {
    /// A mass in grams, milligrams, or micrograms.
    Mass(MeasuredMass),
    /// A volume in millilitres.
    Volume(MeasuredVolume),
}

impl MeasuredQuantity {
    /// Parses a measured quantity such as `"30 g"` or `"250 ml"`.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError`] when the string is malformed or qualified, the
    /// unit is not a supported mass or volume unit, or the amount is not
    /// positive.
    pub fn parse(text: &str) -> Result<Self, MeasureError> {
        let (qualifier, value, unit) = split_reported(text)?;
        reject_qualifier(qualifier, text)?;
        Self::from_parts(&unit, value)
    }

    /// Builds a measured quantity from a unit name and exact decimal.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError`] when the unit is not a supported mass or volume
    /// unit, or the amount is not positive.
    pub fn from_parts(unit: &str, value: Decimal) -> Result<Self, MeasureError> {
        match MassUnit::from_token(unit) {
            Some(unit) => Ok(Self::Mass(MeasuredMass::new(unit, value)?)),
            None if unit_matches(unit, MILLILITRE) => Ok(Self::Volume(MeasuredVolume::new(value)?)),
            None => Err(MeasureError::UnsupportedUnit {
                unit: unit.to_owned(),
            }),
        }
    }

    /// Returns the exact measured amount.
    pub fn value(&self) -> Decimal {
        match self {
            Self::Mass(mass) => mass.value(),
            Self::Volume(volume) => volume.value(),
        }
    }
}

impl fmt::Display for MeasuredQuantity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mass(mass) => mass.fmt(formatter),
            Self::Volume(volume) => volume.fmt(formatter),
        }
    }
}

bearer_string!(MeasuredQuantity);

/// Errors produced while parsing or building a measurement quantity.
#[derive(Debug, thiserror::Error)]
pub enum MeasureError {
    /// The string is not a `[qualifier] amount unit` quantity.
    #[error("invalid quantity `{text}`")]
    Malformed {
        /// The rejected input.
        text: String,
    },
    /// The unit is not supported for this quantity type.
    #[error("unsupported unit `{unit}`")]
    UnsupportedUnit {
        /// The rejected unit token.
        unit: String,
    },
    /// The amount is not an exact representable fixed-point decimal.
    #[error("invalid amount: {source}")]
    Amount {
        /// The decimal parse failure.
        #[source]
        source: rust_decimal::Error,
    },
    /// An arithmetic conversion does not fit the fixed-point decimal range.
    #[error("conversion overflowed the fixed-point decimal range")]
    ConversionOverflow,
    /// A measured quantity must be positive.
    #[error("measured quantity must be positive, found {found}")]
    NotPositive {
        /// The rejected amount.
        found: Decimal,
    },
    /// A measured quantity cannot carry a comparison or approximation.
    #[error("measured quantity cannot be qualified: `{text}`")]
    Qualified {
        /// The rejected input.
        text: String,
    },
}

/// Parses a JSON number into an exact decimal, using the number's raw text.
///
/// # Errors
///
/// Returns [`MeasureError::Amount`] when the number's precision or range
/// exceeds the fixed-point decimal representation.
pub(crate) fn decimal_from_json_number(number: &JsonNumber) -> Result<Decimal, MeasureError> {
    parse_decimal(number.as_str())
}

/// Parses a measured quantity's amount from a JSON number.
///
/// Serving and reference quantities are household measures, and USDA's Survey
/// archive serializes some of them from binary floating point, as in
/// `30.800000000000000710542735760100185871124267578125`. A spelling beyond
/// the fixed-point precision is rounded half to even to two decimal places
/// and normalized; every representable value parses exactly with its source
/// scale. Nutrient amounts never use this policy: they stay exact or fail
/// with [`MeasureError::Amount`].
///
/// # Errors
///
/// Returns [`MeasureError::Amount`] when the number cannot be parsed at all.
pub(crate) fn measured_decimal_from_json_number(
    number: &JsonNumber,
) -> Result<Decimal, MeasureError> {
    if let Ok(decimal) = decimal_from_json_number(number) {
        return Ok(decimal);
    }
    Decimal::from_str(number.as_str())
        .map(|decimal| decimal.round_dp(MEASURED_SCALE).normalize())
        .map_err(|source| MeasureError::Amount { source })
}

/// Returns whether a unit token names the canonical unit.
pub(crate) fn unit_matches(token: &str, canonical: &str) -> bool {
    canonical_unit(token) == Some(canonical)
}

const GRAM: &str = "g";
const MILLIGRAM: &str = "mg";
const MICROGRAM: &str = "µg";
const MILLILITRE: &str = "ml";
const KILOCALORIE: &str = "kcal";
const KILOJOULE: &str = "kJ";
/// Decimal places of the documented rounding policy for measured quantities
/// whose source spelling exceeds the fixed-point precision.
const MEASURED_SCALE: u32 = 2;

/// Maps a unit token to its canonical spelling; microgram synonyms
/// (`µg`, `μg`, `ug`) normalize to `µg`.
fn canonical_unit(token: &str) -> Option<&'static str> {
    match token {
        GRAM => Some(GRAM),
        MILLIGRAM => Some(MILLIGRAM),
        "µg" | "μg" | "ug" => Some(MICROGRAM),
        MILLILITRE => Some(MILLILITRE),
        KILOCALORIE => Some(KILOCALORIE),
        KILOJOULE => Some(KILOJOULE),
        _ => None,
    }
}

/// Splits a reported-amount string into qualifier, amount, and unit.
fn split_reported(text: &str) -> Result<(Option<Qualifier>, Decimal, String), MeasureError> {
    let mut tokens = text.split_whitespace();
    let (qualifier, amount, unit) =
        match (tokens.next(), tokens.next(), tokens.next(), tokens.next()) {
            (Some(amount), Some(unit), None, None) => (None, amount, unit),
            (Some(first), Some(amount), Some(unit), None)
                if Qualifier::parse_token(first).is_some() =>
            {
                (Qualifier::parse_token(first), amount, unit)
            }
            _ => {
                return Err(MeasureError::Malformed {
                    text: text.to_owned(),
                });
            }
        };
    if canonical_unit(unit).is_none() {
        return Err(MeasureError::UnsupportedUnit {
            unit: unit.to_owned(),
        });
    }
    let value = parse_decimal(amount)?;
    Ok((qualifier, value, unit.to_owned()))
}

/// Rejects qualifiers on measured quantities.
fn reject_qualifier(qualifier: Option<Qualifier>, text: &str) -> Result<(), MeasureError> {
    match qualifier {
        Some(_) => Err(MeasureError::Qualified {
            text: text.to_owned(),
        }),
        None => Ok(()),
    }
}

/// Parses an exact fixed-point decimal, supporting scientific notation.
///
/// Plain decimals must fit the representation exactly. Exponent notation is
/// first shifted into its plain decimal form, so the same exactness rule
/// applies to the shifted value; a spelling that does not fit is rejected
/// instead of rounded, which a direct scientific parse would silently do.
fn parse_decimal(text: &str) -> Result<Decimal, MeasureError> {
    if text.contains('e') || text.contains('E') {
        Decimal::from_str_exact(&shift_scientific(text)?)
    } else {
        Decimal::from_str_exact(text)
    }
    .map_err(|source| MeasureError::Amount { source })
}

/// Longest shifted digit string built before rejecting the value; any longer
/// integer cannot fit the 96-bit mantissa and any larger scale cannot fit the
/// 28 fractional digits.
const MAX_SHIFTED_DIGITS: i64 = 64;

/// Shifts a scientific-notation spelling into its plain decimal form.
///
/// `4.10E+3` becomes `4100` and `5e-2` becomes `0.05`, with the sign kept.
/// Extreme exponents are rejected before strings are built, so no input can
/// force unbounded output.
fn shift_scientific(text: &str) -> Result<String, MeasureError> {
    let (coefficient, exponent) = text
        .split_once('e')
        .or_else(|| text.split_once('E'))
        .ok_or_else(|| MeasureError::Malformed {
            text: text.to_owned(),
        })?;
    let (sign, coefficient) = match coefficient.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", coefficient.strip_prefix('+').unwrap_or(coefficient)),
    };
    let (integer, fraction) = match coefficient.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (coefficient, ""),
    };
    let decimal_digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    if !decimal_digits(integer) || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return Err(MeasureError::Malformed {
            text: text.to_owned(),
        });
    }
    let exponent = parse_exponent(exponent)?;
    let digits = format!("{integer}{fraction}");
    let shift = exponent - fraction.len() as i64;
    if shift >= 0 {
        // The point moves past the digits; the result is an integer.
        if digits.len() as i64 + shift > MAX_SHIFTED_DIGITS {
            return Err(out_of_range());
        }
        return Ok(format!("{sign}{digits}{}", "0".repeat(shift as usize)));
    }
    // The point moves left, within or before the digits.
    let point = digits.len() as i64 + shift;
    if -shift > MAX_SHIFTED_DIGITS {
        return Err(out_of_range());
    }
    if point > 0 {
        let (head, tail) = digits.split_at(point as usize);
        return Ok(format!("{sign}{head}.{tail}"));
    }
    Ok(format!("{sign}0.{}{digits}", "0".repeat((-point) as usize)))
}

/// Parses a scientific exponent, rejecting absurd magnitudes early.
fn parse_exponent(text: &str) -> Result<i64, MeasureError> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(MeasureError::Malformed {
            text: text.to_owned(),
        });
    }
    let magnitude = digits
        .parse::<i64>()
        .map_err(|_| out_of_range())
        .and_then(|magnitude| {
            (magnitude <= MAX_SHIFTED_DIGITS)
                .then_some(magnitude)
                .ok_or_else(out_of_range)
        })?;
    Ok(if negative { -magnitude } else { magnitude })
}

/// Returns the out-of-range amount error for unrepresentable shifts.
fn out_of_range() -> MeasureError {
    MeasureError::Amount {
        source: rust_decimal::Error::ExceedsMaximumPossibleValue,
    }
}

/// Deserializes a quantity from its unit-bearing string form.
struct BearerVisitor<T>(fn(&str) -> Result<T, MeasureError>);

impl<'de, T> Visitor<'de> for BearerVisitor<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a unit-bearing quantity string")
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<T, E> {
        (self.0)(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[path = "../unit-tests/measure.rs"]
mod tests;
