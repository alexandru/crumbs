//! Shared, unit-aware nutrition data and fixed-point measurements.
//!
//! Values use the supported range `0..=340282366920938463463.374607431768211455`
//! with 18 fractional places. Parsing and normalization are exact-only and never
//! round. Unsupported precision or nonrepresentable ratios return typed
//! `MeasurementError` values, including `PrecisionLoss`.

use std::fmt;
use std::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Nutrition data and serving sizes attached to one food.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParsedFoodData {
    /// Nutrient values, when available.
    pub nutrition: Option<NutritionProfile>,
    /// Declared serving sizes.
    pub serving_sizes: Vec<ServingSize>,
}

/// Basis used for values in a nutrition profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NutritionBasis {
    /// Values are reported per 100 grams.
    #[serde(rename = "100 g")]
    Per100Grams,
    /// Values are reported per 100 millilitres.
    #[serde(rename = "100 ml")]
    Per100Millilitres,
}

/// Nutrient values expressed on one declared basis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NutritionProfile {
    /// Basis for the reported nutrient values.
    pub per: NutritionBasis,
    /// Fat amounts.
    #[serde(default)]
    pub fat: Fat,
    /// Carbohydrate amounts and their fibre convention.
    #[serde(default)]
    pub carbohydrates: Carbohydrates,
    /// Fibre amount, when reported separately.
    pub fibre: Option<Reported<Mass>>,
    /// Protein amount, when reported.
    pub protein: Option<Reported<Mass>>,
    /// Sodium amount, when reported.
    pub sodium: Option<Reported<Mass>>,
    /// Salt amount, when reported.
    pub salt: Option<Reported<Mass>>,
    /// Cholesterol amount, when reported.
    pub cholesterol: Option<Reported<Mass>>,
    /// Energy amount, when reported.
    pub energy: Option<Reported<Energy>>,
}

/// Fat amounts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fat {
    /// Total fat amount.
    pub total: Option<Reported<Mass>>,
    /// Saturated fat amount.
    pub saturated: Option<Reported<Mass>>,
    /// Monounsaturated fat amount.
    pub monounsaturated: Option<Reported<Mass>>,
    /// Polyunsaturated fat amount.
    pub polyunsaturated: Option<Reported<Mass>>,
    /// Trans fat amount.
    pub trans: Option<Reported<Mass>>,
}

/// Carbohydrate amounts with explicit fibre inclusion semantics.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carbohydrates {
    /// Amount that includes fibre, when reported in that convention.
    pub including_fibre: Option<Reported<Mass>>,
    /// Amount that excludes fibre, when reported in that convention.
    pub excluding_fibre: Option<Reported<Mass>>,
    /// Sugar amounts.
    #[serde(default)]
    pub sugars: Sugars,
    /// Starch amount, when reported.
    pub starch: Option<Reported<Mass>>,
}

/// Sugar amounts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sugars {
    /// Total sugars amount.
    pub total: Option<Reported<Mass>>,
    /// Added sugars amount.
    pub added: Option<Reported<Mass>>,
    /// Lactose amount.
    pub lactose: Option<Reported<Mass>>,
}

/// Mass measurements whose unit is part of the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mass {
    /// A value in grams.
    Grams(Grams),
    /// A value in milligrams.
    Milligrams(Milligrams),
    /// A value in micrograms.
    Micrograms(Micrograms),
}

/// Energy measurements whose unit is part of the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Energy {
    /// A value in kilocalories.
    Kilocalories(Kilocalories),
    /// A value in kilojoules.
    Kilojoules(Kilojoules),
}

/// A reported value and its qualifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reported<T> {
    amount: T,
    qualifier: Qualifier,
}

/// Qualifier attached to a reported measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Qualifier {
    /// No comparison or approximation was supplied.
    #[serde(rename = "exact")]
    Exact,
    /// The amount is less than the reported value.
    #[serde(rename = "<")]
    LessThan,
    /// The amount is less than or equal to the reported value.
    #[serde(rename = "<=")]
    LessThanOrEqual,
    /// The amount is greater than the reported value.
    #[serde(rename = ">")]
    GreaterThan,
    /// The amount is greater than or equal to the reported value.
    #[serde(rename = ">=")]
    GreaterThanOrEqual,
    /// The amount is approximate.
    #[serde(rename = "~")]
    Approximate,
}

/// A serving quantity with a positive mass or volume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServingQuantity(ServingAmount);

/// A human-readable serving name.
pub type ServingName = String;

/// A named declared serving size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServingSize {
    /// Human-readable serving name.
    pub name: ServingName,
    /// Positive mass or volume for the serving.
    pub quantity: ServingQuantity,
}

/// Exact, nonnegative decimal arithmetic errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MeasurementError {
    /// The input is not a supported decimal number.
    #[error("invalid decimal value")]
    InvalidDecimal,
    /// The input has no supported unit or uses a different unit than required.
    #[error("invalid measurement unit")]
    InvalidUnit,
    /// The input unit belongs to a different measurement dimension.
    #[error("incompatible measurement dimension")]
    IncompatibleDimension,
    /// An exact result cannot be represented at the supported precision.
    #[error("measurement precision would be lost")]
    PrecisionLoss,
    /// The value exceeds the supported numeric range.
    #[error("measurement is outside the supported range")]
    OutOfRange,
    /// A serving quantity must be greater than zero.
    #[error("serving quantity must be positive")]
    NonPositive,
    /// A serving quantity cannot carry a qualifier.
    #[error("serving quantity cannot have a qualifier")]
    QualifierNotAllowed,
    /// An exact measurement conversion could not be performed.
    #[error("measurement conversion failed")]
    ConversionFailed,
}

/// A gram quantity with a private fixed-point representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grams(Decimal);

/// A milligram quantity with a private fixed-point representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Milligrams(Decimal);

/// A microgram quantity with a private fixed-point representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Micrograms(Decimal);

/// A kilocalorie quantity with a private fixed-point representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kilocalories(Decimal);

/// A kilojoule quantity with a private fixed-point representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kilojoules(Decimal);

/// A millilitre quantity with a private fixed-point representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Millilitres(Decimal);

impl NutritionProfile {
    /// Creates a profile with the given basis and no reported nutrient amounts.
    pub fn new(per: NutritionBasis) -> Self {
        Self {
            per,
            fat: Fat::default(),
            carbohydrates: Carbohydrates::default(),
            fibre: None,
            protein: None,
            sodium: None,
            salt: None,
            cholesterol: None,
            energy: None,
        }
    }
}

impl<T> Reported<T> {
    /// Creates a reported value with the supplied qualifier.
    pub fn new(amount: T, qualifier: Qualifier) -> Self {
        Self { amount, qualifier }
    }

    /// Returns the measured amount.
    pub fn amount(&self) -> &T {
        &self.amount
    }

    /// Returns the qualifier for the measured amount.
    pub fn qualifier(&self) -> Qualifier {
        self.qualifier
    }
}

impl FromStr for Mass {
    type Err = MeasurementError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (amount, unit) = parse_unit_amount(input)?;
        match unit {
            Unit::Grams => Ok(Self::Grams(Grams(amount))),
            Unit::Milligrams => Ok(Self::Milligrams(Milligrams(amount))),
            Unit::Micrograms => Ok(Self::Micrograms(Micrograms(amount))),
            Unit::Kilocalories | Unit::Kilojoules | Unit::Millilitres => {
                Err(MeasurementError::IncompatibleDimension)
            }
        }
    }
}

impl FromStr for Energy {
    type Err = MeasurementError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (amount, unit) = parse_unit_amount(input)?;
        match unit {
            Unit::Kilocalories => Ok(Self::Kilocalories(Kilocalories(amount))),
            Unit::Kilojoules => Ok(Self::Kilojoules(Kilojoules(amount))),
            Unit::Grams | Unit::Milligrams | Unit::Micrograms | Unit::Millilitres => {
                Err(MeasurementError::IncompatibleDimension)
            }
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

impl fmt::Display for Energy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kilocalories(value) => fmt::Display::fmt(value, formatter),
            Self::Kilojoules(value) => fmt::Display::fmt(value, formatter),
        }
    }
}

impl<T> fmt::Display for Reported<T>
where
    T: fmt::Display,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.qualifier {
            Qualifier::Exact => fmt::Display::fmt(&self.amount, formatter),
            qualifier => write!(formatter, "{} {}", qualifier.symbol(), self.amount),
        }
    }
}

impl FromStr for ServingQuantity {
    type Err = MeasurementError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if matches!(input.chars().next(), Some('<' | '>' | '~')) {
            return Err(MeasurementError::QualifierNotAllowed);
        }
        let (amount, unit) = parse_unit_amount(input)?;
        let serving = match unit {
            Unit::Grams => ServingAmount::Mass(Mass::Grams(Grams(amount))),
            Unit::Milligrams => ServingAmount::Mass(Mass::Milligrams(Milligrams(amount))),
            Unit::Micrograms => ServingAmount::Mass(Mass::Micrograms(Micrograms(amount))),
            Unit::Millilitres => ServingAmount::Volume(Millilitres(amount)),
            Unit::Kilocalories | Unit::Kilojoules => {
                return Err(MeasurementError::IncompatibleDimension);
            }
        };
        if serving.is_zero() {
            return Err(MeasurementError::NonPositive);
        }
        Ok(Self(serving))
    }
}

impl fmt::Display for ServingQuantity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ServingAmount::Mass(mass) => fmt::Display::fmt(&mass, formatter),
            ServingAmount::Volume(millilitres) => fmt::Display::fmt(&millilitres, formatter),
        }
    }
}

impl<T> FromStr for Reported<T>
where
    T: FromStr<Err = MeasurementError>,
{
    type Err = MeasurementError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        parse_reported(input)
    }
}

impl<T> Serialize for Reported<T>
where
    T: fmt::Display,
{
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
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(D::Error::custom)
    }
}

macro_rules! impl_unit_measurement {
    ($type:ty, $unit:literal, $expected:ident) => {
        impl FromStr for $type {
            type Err = MeasurementError;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                let (amount, unit) = parse_unit_amount(input)?;
                if unit == Unit::$expected {
                    Ok(Self(amount))
                } else {
                    let same_dimension = match $unit {
                        "g" | "mg" | "µg" => {
                            matches!(unit, Unit::Grams | Unit::Milligrams | Unit::Micrograms)
                        }
                        "kcal" | "kJ" => matches!(unit, Unit::Kilocalories | Unit::Kilojoules),
                        "ml" => matches!(unit, Unit::Millilitres),
                        _ => false,
                    };
                    if same_dimension {
                        Err(MeasurementError::InvalidUnit)
                    } else {
                        Err(MeasurementError::IncompatibleDimension)
                    }
                }
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{} {}", self.0, $unit)
            }
        }

        impl_string_serde!($type);
    };
}

macro_rules! impl_string_serde {
    ($type:ty) => {
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
                let value = String::deserialize(deserializer)?;
                value.parse().map_err(D::Error::custom)
            }
        }
    };
}

impl_unit_measurement!(Grams, "g", Grams);
impl_unit_measurement!(Milligrams, "mg", Milligrams);
impl_unit_measurement!(Micrograms, "µg", Micrograms);
impl_unit_measurement!(Kilocalories, "kcal", Kilocalories);
impl_unit_measurement!(Kilojoules, "kJ", Kilojoules);
impl_unit_measurement!(Millilitres, "ml", Millilitres);

impl_string_serde!(Mass);
impl_string_serde!(Energy);
impl_string_serde!(ServingQuantity);

/// A nonnegative fixed-point decimal with 18 fractional places.
///
/// Values are stored as an unsigned integer scaled by 10^18. The supported
/// range is `0` through `340282366920938463463.374607431768211455`; parsing
/// never uses floating point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Decimal(u128);

const DECIMAL_SCALE: u128 = 1_000_000_000_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Grams,
    Milligrams,
    Micrograms,
    Kilocalories,
    Kilojoules,
    Millilitres,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServingAmount {
    Mass(Mass),
    Volume(Millilitres),
}

impl ServingAmount {
    fn is_zero(self) -> bool {
        match self {
            Self::Mass(Mass::Grams(value)) => value.0.is_zero(),
            Self::Mass(Mass::Milligrams(value)) => value.0.is_zero(),
            Self::Mass(Mass::Micrograms(value)) => value.0.is_zero(),
            Self::Volume(value) => value.0.is_zero(),
        }
    }
}

impl Mass {
    fn checked_ratio(
        self,
        numerator: Decimal,
        denominator: Decimal,
    ) -> Result<Self, MeasurementError> {
        match self {
            Self::Grams(value) => Ok(Self::Grams(Grams(
                value.0.checked_ratio(numerator, denominator)?,
            ))),
            Self::Milligrams(value) => Ok(Self::Milligrams(Milligrams(
                value.0.checked_ratio(numerator, denominator)?,
            ))),
            Self::Micrograms(value) => Ok(Self::Micrograms(Micrograms(
                value.0.checked_ratio(numerator, denominator)?,
            ))),
        }
    }
}

impl Energy {
    fn checked_ratio(
        self,
        numerator: Decimal,
        denominator: Decimal,
    ) -> Result<Self, MeasurementError> {
        match self {
            Self::Kilocalories(value) => Ok(Self::Kilocalories(Kilocalories(
                value.0.checked_ratio(numerator, denominator)?,
            ))),
            Self::Kilojoules(value) => Ok(Self::Kilojoules(Kilojoules(
                value.0.checked_ratio(numerator, denominator)?,
            ))),
        }
    }
}

impl Reported<Mass> {
    /// Scales the amount exactly while preserving its unit and qualifier.
    pub(crate) fn checked_ratio(
        &self,
        numerator: Decimal,
        denominator: Decimal,
    ) -> Result<Self, MeasurementError> {
        let amount = self.amount.checked_ratio(numerator, denominator)?;
        Ok(Self::new(amount, self.qualifier))
    }
}

impl Reported<Energy> {
    /// Scales the amount exactly while preserving its unit and qualifier.
    pub(crate) fn checked_ratio(
        &self,
        numerator: Decimal,
        denominator: Decimal,
    ) -> Result<Self, MeasurementError> {
        let amount = self.amount.checked_ratio(numerator, denominator)?;
        Ok(Self::new(amount, self.qualifier))
    }
}

impl ServingQuantity {
    /// Returns the nutrition basis and exact ratio for 100 units of this serving.
    pub(crate) fn hundred_ratio(&self) -> Result<(Decimal, Decimal), MeasurementError> {
        let amount = match self.0 {
            ServingAmount::Mass(Mass::Grams(grams)) => grams.0,
            ServingAmount::Mass(Mass::Milligrams(milligrams)) => milligrams
                .0
                .checked_ratio(Decimal::one(), Decimal::from_integer(1_000)?)?,
            ServingAmount::Mass(Mass::Micrograms(micrograms)) => micrograms
                .0
                .checked_ratio(Decimal::one(), Decimal::from_integer(1_000_000)?)?,
            ServingAmount::Volume(millilitres) => millilitres.0,
        };
        Ok((Decimal::from_integer(100)?, amount))
    }

    /// Returns whether this serving is mass-based or volume-based.
    pub(crate) const fn basis(&self) -> NutritionBasis {
        match self.0 {
            ServingAmount::Mass(_) => NutritionBasis::Per100Grams,
            ServingAmount::Volume(_) => NutritionBasis::Per100Millilitres,
        }
    }
}

impl FromStr for Decimal {
    type Err = MeasurementError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        parse_decimal(input)
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / DECIMAL_SCALE;
        let fraction = self.0 % DECIMAL_SCALE;
        if fraction == 0 {
            return write!(formatter, "{whole}");
        }

        let mut fractional = format!("{fraction:018}");
        while fractional.ends_with('0') {
            fractional.pop();
        }
        write!(formatter, "{whole}.{fractional}")
    }
}

impl Decimal {
    /// Returns an exact ratio or a typed error when its result is not representable.
    pub(crate) fn checked_ratio(
        self,
        numerator: Decimal,
        denominator: Decimal,
    ) -> Result<Decimal, MeasurementError> {
        if denominator.is_zero() {
            return Err(MeasurementError::ConversionFailed);
        }

        let mut left = self.0;
        let mut right = numerator.0;
        let mut divisor = denominator.0;

        let common_left = greatest_common_divisor(left, divisor);
        left /= common_left;
        divisor /= common_left;
        let common_right = greatest_common_divisor(right, divisor);
        right /= common_right;
        divisor /= common_right;

        // A reduced denominator above one means the exact tick count is fractional.
        if divisor != 1 {
            return Err(MeasurementError::PrecisionLoss);
        }
        left.checked_mul(right)
            .map(Self)
            .ok_or(MeasurementError::OutOfRange)
    }

    /// Returns whether this decimal is zero.
    pub(crate) const fn is_zero(self) -> bool {
        self.0 == 0
    }

    fn one() -> Self {
        Self(DECIMAL_SCALE)
    }

    fn from_integer(value: u128) -> Result<Self, MeasurementError> {
        value
            .checked_mul(DECIMAL_SCALE)
            .map(Self)
            .ok_or(MeasurementError::OutOfRange)
    }
}

impl Qualifier {
    fn symbol(self) -> &'static str {
        match self {
            Self::Exact => "",
            Self::LessThan => "<",
            Self::LessThanOrEqual => "<=",
            Self::GreaterThan => ">",
            Self::GreaterThanOrEqual => ">=",
            Self::Approximate => "~",
        }
    }
}

fn parse_reported<T>(input: &str) -> Result<Reported<T>, MeasurementError>
where
    T: FromStr<Err = MeasurementError>,
{
    let input = input.trim();
    let (qualifier, amount) = if let Some(amount) =
        input.strip_prefix("<=").or_else(|| input.strip_prefix('≤'))
    {
        (Qualifier::LessThanOrEqual, amount)
    } else if let Some(amount) = input.strip_prefix(">=").or_else(|| input.strip_prefix('≥')) {
        (Qualifier::GreaterThanOrEqual, amount)
    } else if let Some(amount) = input.strip_prefix('<') {
        (Qualifier::LessThan, amount)
    } else if let Some(amount) = input.strip_prefix('>') {
        (Qualifier::GreaterThan, amount)
    } else if let Some(amount) = input.strip_prefix('~') {
        (Qualifier::Approximate, amount)
    } else {
        (Qualifier::Exact, input)
    };
    let amount = amount.trim_start();
    if amount.is_empty() {
        return Err(MeasurementError::InvalidDecimal);
    }
    Ok(Reported::new(amount.parse()?, qualifier))
}

fn parse_unit_amount(input: &str) -> Result<(Decimal, Unit), MeasurementError> {
    let input = input.trim();
    for (suffix, unit) in [
        ("kcal", Unit::Kilocalories),
        ("kJ", Unit::Kilojoules),
        ("mg", Unit::Milligrams),
        ("µg", Unit::Micrograms),
        ("ug", Unit::Micrograms),
        ("ml", Unit::Millilitres),
        ("g", Unit::Grams),
    ] {
        if let Some(number) = input.strip_suffix(suffix) {
            return Ok((number.trim().parse()?, unit));
        }
    }

    let number_end = decimal_prefix_length(input).ok_or(MeasurementError::InvalidDecimal)?;
    let unit_text = input[number_end..].trim();
    if unit_text.is_empty() {
        return Err(MeasurementError::InvalidUnit);
    }
    input[..number_end].parse::<Decimal>()?;
    Err(MeasurementError::InvalidUnit)
}

fn decimal_prefix_length(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut position = 0;
    if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        position += 1;
    }
    let integer_start = position;
    while matches!(bytes.get(position), Some(b'0'..=b'9')) {
        position += 1;
    }
    if position == integer_start {
        return None;
    }
    if bytes.get(position) == Some(&b'.') {
        position += 1;
        let fraction_start = position;
        while matches!(bytes.get(position), Some(b'0'..=b'9')) {
            position += 1;
        }
        if position == fraction_start {
            return None;
        }
    }
    if matches!(bytes.get(position), Some(b'e') | Some(b'E')) {
        position += 1;
        if matches!(bytes.get(position), Some(b'+') | Some(b'-')) {
            position += 1;
        }
        let exponent_start = position;
        while matches!(bytes.get(position), Some(b'0'..=b'9')) {
            position += 1;
        }
        if position == exponent_start {
            return None;
        }
    }
    Some(position)
}

fn parse_decimal(input: &str) -> Result<Decimal, MeasurementError> {
    let (mantissa, exponent_text) = match input.find('e').or_else(|| input.find('E')) {
        Some(position) => (&input[..position], Some(&input[position + 1..])),
        None => (input, None),
    };
    if input
        .bytes()
        .filter(|byte| matches!(*byte, b'e' | b'E'))
        .count()
        > 1
    {
        return Err(MeasurementError::InvalidDecimal);
    }

    let mantissa = match mantissa.strip_prefix('+') {
        Some(value) => value,
        None => mantissa,
    };
    if mantissa.starts_with('-') {
        return Err(MeasurementError::InvalidDecimal);
    }
    let exponent = match exponent_text {
        Some(value) => parse_exponent(value)?,
        None => 0,
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) => {
            if whole.is_empty()
                || fraction.is_empty()
                || !whole.bytes().all(|byte| byte.is_ascii_digit())
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(MeasurementError::InvalidDecimal);
            }
            (whole, fraction)
        }
        None => {
            if mantissa.is_empty() || !mantissa.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(MeasurementError::InvalidDecimal);
            }
            (mantissa, "")
        }
    };

    let mut digits = String::with_capacity(whole.len().saturating_add(fraction.len()));
    digits.push_str(whole);
    digits.push_str(fraction);
    let fraction_length =
        i128::try_from(fraction.len()).map_err(|_| MeasurementError::OutOfRange)?;
    let shift = i128::from(exponent)
        .checked_add(18)
        .and_then(|value| value.checked_sub(fraction_length))
        .ok_or(MeasurementError::OutOfRange)?;

    if digits.bytes().all(|byte| byte == b'0') {
        return Ok(Decimal(0));
    }

    if shift >= 0 {
        let significant = digits.trim_start_matches('0');
        let shift = usize::try_from(shift).map_err(|_| MeasurementError::OutOfRange)?;
        let result_length = significant
            .len()
            .checked_add(shift)
            .ok_or(MeasurementError::OutOfRange)?;
        if result_length > 39 {
            return Err(MeasurementError::OutOfRange);
        }
        let mut scaled = String::with_capacity(result_length);
        scaled.push_str(significant);
        scaled.extend(std::iter::repeat_n('0', shift));
        return scaled
            .parse::<u128>()
            .map(Decimal)
            .map_err(|_| MeasurementError::OutOfRange);
    }

    let removed = shift.unsigned_abs();
    if removed >= u128::try_from(digits.len()).map_err(|_| MeasurementError::OutOfRange)? {
        return Err(MeasurementError::PrecisionLoss);
    }
    let removed = usize::try_from(removed).map_err(|_| MeasurementError::OutOfRange)?;
    let retained_length = digits.len() - removed;
    if !digits[retained_length..].bytes().all(|byte| byte == b'0') {
        return Err(MeasurementError::PrecisionLoss);
    }
    let retained = digits[..retained_length].trim_start_matches('0');
    if retained.is_empty() {
        return Ok(Decimal(0));
    }
    retained
        .parse::<u128>()
        .map(Decimal)
        .map_err(|_| MeasurementError::OutOfRange)
}

fn parse_exponent(input: &str) -> Result<i64, MeasurementError> {
    let digits = match input.strip_prefix('+').or_else(|| input.strip_prefix('-')) {
        Some(digits) => digits,
        None => input,
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(MeasurementError::InvalidDecimal);
    }
    input
        .parse::<i64>()
        .map_err(|_| MeasurementError::OutOfRange)
}

fn greatest_common_divisor(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[cfg(test)]
#[path = "../unit-tests/nutrition.rs"]
mod tests;
