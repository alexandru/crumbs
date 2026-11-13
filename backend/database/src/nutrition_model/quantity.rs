use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use rust_decimal::Decimal;
use rust_decimal::dec;
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

/// A decimal measurement in its retained source unit.
pub trait Measurement: sealed::Sealed + Copy + fmt::Display {
    /// Returns the decimal value in the retained source unit.
    fn value(self) -> Decimal;

    /// Returns the unit symbol for this measurement.
    fn unit_symbol(self) -> Cow<'static, str>;

    /// Returns whether the measurement is zero.
    fn is_zero(self) -> bool {
        self.value().is_zero()
    }

    /// Returns whether this measurement is negative.
    fn is_negative(self) -> bool {
        self.value().is_sign_negative() && !self.value().is_zero()
    }

    /// Returns whether this measurement is positive.
    fn is_positive(self) -> bool {
        self.value().is_sign_positive() && !self.value().is_zero()
    }

    /// Parses a decimal value and its unit without first formatting them together.
    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError>;
}

/// A measurement type with one fixed unit symbol.
pub trait FixedUnit: Measurement {
    /// The symbol for this unit.
    const SYMBOL: &'static str;
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
        if amount.is_negative() {
            return Err(QuantityError::Negative);
        }
        if qualifier == Qualifier::LessThan && amount.is_zero() {
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

macro_rules! unit_numeric_conversions {
    ($name:ident; $($number:ty),+ $(,)?) => {
        impl From<Decimal> for $name {
            fn from(value: Decimal) -> Self { Self::new(value) }
        }
        $(impl From<$number> for $name {
            fn from(value: $number) -> Self { Self::new(Decimal::from(value)) }
        })+
    };
}

macro_rules! ratio_integer_conversions {
    ($($number:ty),+ $(,)?) => {
        $(impl<N: From<$number>, D> From<$number> for Ratio<N, D> {
            fn from(value: $number) -> Self { Self::new(N::from(value)) }
        })+
    };
}

// Each concrete unit owns its invariant; dimension enums cannot contain raw decimals.
macro_rules! measurement {
    ($name:ident, $unit:literal) => {
        #[doc = concat!("An exact signed decimal quantity in ", $unit, ".")]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        pub struct $name(Decimal);

        unit_numeric_conversions!($name; i8, i16, i32, i64, u8, u16, u32, u64);

        impl $name {
            /// Zero in this unit.
            pub const ZERO: Self = Self(Decimal::ZERO);

            /// One in this unit.
            pub const ONE: Self = Self(Decimal::ONE);

            /// Creates a quantity from an exact fixed-point value.
            pub const fn new(value: Decimal) -> Self {
                Self(value)
            }

            /// Returns whether the value is zero.
            pub fn is_zero(self) -> bool {
                self.0.is_zero()
            }

            /// Returns whether the value is negative.
            pub fn is_negative(self) -> bool {
                self.0.is_sign_negative() && !self.is_zero()
            }

            /// Returns whether the value is positive.
            pub fn is_positive(self) -> bool {
                self.0.is_sign_positive() && !self.is_zero()
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

        impl FixedUnit for $name {
            const SYMBOL: &'static str = $unit;
        }

        impl Measurement for $name {
            fn value(self) -> Decimal {
                self.0
            }

            fn unit_symbol(self) -> Cow<'static, str> {
                Cow::Borrowed(Self::SYMBOL)
            }

            fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
                if unit != $unit {
                    return Err(QuantityError::Unit(unit.to_owned()));
                }
                Ok(Self::new(parse_decimal(value)?))
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
        impl std::ops::Add for $name {
            type Output = Self;
            fn add(self, other: Self) -> Self {
                Self(self.0 + other.0)
            }
        }

        impl std::ops::Sub for $name {
            type Output = Self;
            fn sub(self, other: Self) -> Self {
                Self(self.0 - other.0)
            }
        }

        impl std::ops::Mul<Decimal> for $name {
            type Output = Self;
            fn mul(self, other: Decimal) -> Self {
                Self(self.0 * other)
            }
        }

        impl std::ops::Div<Decimal> for $name {
            type Output = Self;
            fn div(self, other: Decimal) -> Self {
                Self(self.0 / other)
            }
        }

        impl std::ops::Neg for $name {
            type Output = Self;
            fn neg(self) -> Self {
                Self(-self.0)
            }
        }
    };
}

measurement!(Grams, "g");
measurement!(Milligrams, "mg");
measurement!(Micrograms, "µg");
measurement!(Millilitres, "ml");
measurement!(Litres, "l");
measurement!(Kcal, "kcal");
measurement!(Minutes, "min");
measurement!(Kilojoules, "kJ");
measurement!(Centimetres, "cm");
measurement!(Kilograms, "kg");

/// A numerator quantity per one unit of the denominator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ratio<N, D>(N, PhantomData<D>);

impl<N, D> Ratio<N, D> {
    /// Creates a ratio from the numerator quantity.
    pub const fn new(amount: N) -> Self {
        Self(amount, PhantomData)
    }

    /// Returns the numerator quantity.
    pub fn amount(self) -> N {
        self.0
    }
}

impl<N: From<Decimal>, D> From<Decimal> for Ratio<N, D> {
    fn from(value: Decimal) -> Self {
        Self::new(N::from(value))
    }
}

ratio_integer_conversions!(i8, i16, i32, i64, u8, u16, u32, u64);

impl<N: Measurement, D: FixedUnit> sealed::Sealed for Ratio<N, D> {}
impl<N: Measurement, D: FixedUnit> Measurement for Ratio<N, D> {
    fn value(self) -> Decimal {
        self.0.value()
    }
    fn unit_symbol(self) -> Cow<'static, str> {
        Cow::Owned(format!("{}/{}", self.0.unit_symbol(), D::SYMBOL))
    }
    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
        let (numerator, denominator) = unit
            .rsplit_once('/')
            .ok_or_else(|| QuantityError::Unit(unit.to_owned()))?;
        if denominator != D::SYMBOL {
            return Err(QuantityError::Unit(unit.to_owned()));
        }
        N::parse_with_unit(value, numerator).map(Self::new)
    }
}
impl<N: Measurement, D: FixedUnit> FromStr for Ratio<N, D> {
    type Err = QuantityError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (number, unit) = split_quantity(value)?;
        Self::parse_with_unit(number, unit)
    }
}
impl<N: Measurement, D: FixedUnit> fmt::Display for Ratio<N, D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}/{}",
            self.0.value(),
            self.0.unit_symbol(),
            D::SYMBOL
        )
    }
}
impl<N: Measurement, D: FixedUnit> Serialize for Ratio<N, D> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
impl<'de, N: Measurement, D: FixedUnit> Deserialize<'de> for Ratio<N, D> {
    fn deserialize<Ds: Deserializer<'de>>(deserializer: Ds) -> Result<Self, Ds::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
impl<N, D: FixedUnit> std::ops::Mul<D> for Ratio<N, D>
where
    N: std::ops::Mul<Decimal, Output = N>,
{
    type Output = N;
    fn mul(self, denominator: D) -> N {
        self.0 * denominator.value()
    }
}
impl<N: std::ops::Add<Output = N>, D> std::ops::Add for Ratio<N, D> {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.0 + other.0)
    }
}
impl<N: std::ops::Sub<Output = N>, D> std::ops::Sub for Ratio<N, D> {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.0 - other.0)
    }
}
impl<N: std::ops::Neg<Output = N>, D> std::ops::Neg for Ratio<N, D> {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.0)
    }
}

/// A quantity expressed per week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PerWeek<T>(T);

impl<T> PerWeek<T> {
    /// Creates a weekly rate from its weekly amount.
    pub const fn new(value: T) -> Self {
        Self(value)
    }
    /// Returns the amount for one week.
    pub fn amount(self) -> T {
        self.0
    }
}

impl<T: Measurement> PerWeek<T> {
    /// Returns whether the amount is zero.
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }
    /// Returns whether the amount is negative.
    pub fn is_negative(self) -> bool {
        self.0.is_negative()
    }
    /// Returns whether the amount is positive.
    pub fn is_positive(self) -> bool {
        self.0.is_positive()
    }
}

impl<T> From<T> for PerWeek<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}
impl<T: Measurement> sealed::Sealed for PerWeek<T> {}

impl<T: Measurement> Measurement for PerWeek<T> {
    fn value(self) -> Decimal {
        self.0.value()
    }

    fn unit_symbol(self) -> Cow<'static, str> {
        Cow::Owned(format!("{}/week", self.0.unit_symbol()))
    }

    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
        let base = unit
            .strip_suffix("/week")
            .ok_or_else(|| QuantityError::Unit(unit.to_owned()))?;
        T::parse_with_unit(value, base).map(Self)
    }
}

impl<T: Measurement> FromStr for PerWeek<T> {
    type Err = QuantityError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (number, unit) = split_quantity(value)?;
        Self::parse_with_unit(number, unit)
    }
}
impl<T: Measurement> fmt::Display for PerWeek<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}/week", self.0.value(), self.0.unit_symbol())
    }
}

impl<T: Measurement> Serialize for PerWeek<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de, T: Measurement> Deserialize<'de> for PerWeek<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A quantity expressed per day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PerDay<T>(T);

impl<T> PerDay<T> {
    /// Creates a daily rate from its daily amount.
    pub const fn new(value: T) -> Self {
        Self(value)
    }
    /// Returns the amount for one day.
    pub fn amount(self) -> T {
        self.0
    }
}

impl<T: Measurement> PerDay<T> {
    /// Returns whether the amount is zero.
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }
    /// Returns whether the amount is negative.
    pub fn is_negative(self) -> bool {
        self.0.is_negative()
    }
    /// Returns whether the amount is positive.
    pub fn is_positive(self) -> bool {
        self.0.is_positive()
    }
}

impl<T> From<T> for PerDay<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}
impl<T: Measurement> sealed::Sealed for PerDay<T> {}

impl<T: Measurement> Measurement for PerDay<T> {
    fn value(self) -> Decimal {
        self.0.value()
    }

    fn unit_symbol(self) -> Cow<'static, str> {
        Cow::Owned(format!("{}/day", self.0.unit_symbol()))
    }

    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
        let base = unit
            .strip_suffix("/day")
            .ok_or_else(|| QuantityError::Unit(unit.to_owned()))?;
        T::parse_with_unit(value, base).map(Self)
    }
}

impl<T: Measurement> FromStr for PerDay<T> {
    type Err = QuantityError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (number, unit) = split_quantity(value)?;
        Self::parse_with_unit(number, unit)
    }
}
impl<T: Measurement> fmt::Display for PerDay<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}/day", self.0.value(), self.0.unit_symbol())
    }
}

impl<T: Measurement> Serialize for PerDay<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de, T: Measurement> Deserialize<'de> for PerDay<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
impl<T: std::ops::Add<Output = T>> std::ops::Add for PerDay<T> {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}
impl<T: std::ops::Add<Output = T>> std::ops::Add<T> for PerDay<T> {
    type Output = Self;
    fn add(self, other: T) -> Self {
        Self(self.0 + other)
    }
}
impl<T: std::ops::Sub<Output = T>> std::ops::Sub for PerDay<T> {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}
impl<T: std::ops::Sub<Output = T>> std::ops::Sub<T> for PerDay<T> {
    type Output = Self;
    fn sub(self, other: T) -> Self {
        Self(self.0 - other)
    }
}
impl<T: std::ops::Neg<Output = T>> std::ops::Neg for PerDay<T> {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}
impl<T, Scalar> std::ops::Mul<Scalar> for PerDay<T>
where
    T: std::ops::Mul<Scalar, Output = T>,
{
    type Output = Self;
    fn mul(self, other: Scalar) -> Self {
        Self(self.0 * other)
    }
}

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
    pub fn to_grams(self) -> Grams {
        match self {
            Self::Kilograms(value) => Grams::new(value.value() * dec!(1000)),
            Self::Grams(value) => value,
            Self::Milligrams(value) => Grams::new(value.value() / dec!(1000)),
            Self::Micrograms(value) => Grams::new(value.value() / dec!(1_000_000)),
        }
    }
}

impl sealed::Sealed for Mass {}

impl Measurement for Mass {
    fn value(self) -> Decimal {
        Mass::value(self)
    }

    fn unit_symbol(self) -> Cow<'static, str> {
        match self {
            Self::Kilograms(value) => value.unit_symbol(),
            Self::Grams(value) => value.unit_symbol(),
            Self::Milligrams(value) => value.unit_symbol(),
            Self::Micrograms(value) => value.unit_symbol(),
        }
    }

    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
        match unit {
            "kg" => Ok(Self::Kilograms(Kilograms::new(parse_decimal(value)?))),
            "g" => Ok(Self::Grams(Grams::new(parse_decimal(value)?))),
            "mg" => Ok(Self::Milligrams(Milligrams::new(parse_decimal(value)?))),
            "µg" => Ok(Self::Micrograms(Micrograms::new(parse_decimal(value)?))),
            _ => Err(QuantityError::Unit(unit.to_owned())),
        }
    }
}

impl std::ops::Mul<Decimal> for Mass {
    type Output = Self;

    fn mul(self, factor: Decimal) -> Self {
        match self {
            Self::Kilograms(value) => Self::Kilograms(value * factor),
            Self::Grams(value) => Self::Grams(value * factor),
            Self::Milligrams(value) => Self::Milligrams(value * factor),
            Self::Micrograms(value) => Self::Micrograms(value * factor),
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
    Kcal(Kcal),
    Kilojoules(Kilojoules),
}

impl Energy {
    /// Returns the decimal value in the retained source unit.
    pub fn value(self) -> Decimal {
        match self {
            Self::Kcal(value) => value.value(),
            Self::Kilojoules(value) => value.value(),
        }
    }

    /// Converts this energy to kilocalories using `1 kcal = 4.184 kJ`.
    ///
    /// Arithmetic uses at most 28 fractional decimal places and the `Decimal` 96-bit
    /// coefficient. When precision must be reduced, midpoint ties round to nearest even.
    pub fn to_kilocalories(self) -> Kcal {
        match self {
            Self::Kcal(value) => value,
            Self::Kilojoules(value) => Kcal::new(value.value() / dec!(4.184)),
        }
    }
}

impl sealed::Sealed for Energy {}

impl Measurement for Energy {
    fn value(self) -> Decimal {
        Energy::value(self)
    }

    fn unit_symbol(self) -> Cow<'static, str> {
        match self {
            Self::Kcal(value) => value.unit_symbol(),
            Self::Kilojoules(value) => value.unit_symbol(),
        }
    }

    fn parse_with_unit(value: &str, unit: &str) -> Result<Self, QuantityError> {
        match unit {
            "kcal" => Ok(Self::Kcal(Kcal::new(parse_decimal(value)?))),
            "kJ" => Ok(Self::Kilojoules(Kilojoules::new(parse_decimal(value)?))),
            _ => Err(QuantityError::Unit(unit.to_owned())),
        }
    }
}

impl std::ops::Mul<Decimal> for Energy {
    type Output = Self;

    fn mul(self, factor: Decimal) -> Self {
        match self {
            Self::Kcal(value) => Self::Kcal(value * factor),
            Self::Kilojoules(value) => Self::Kilojoules(value * factor),
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
            Self::Kcal(value) => value.fmt(f),
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
    pub(crate) fn is_positive(&self) -> bool {
        match self {
            Self::Mass(Mass::Kilograms(value)) => value.is_positive(),
            Self::Mass(Mass::Grams(value)) => value.is_positive(),
            Self::Mass(Mass::Milligrams(value)) => value.is_positive(),
            Self::Mass(Mass::Micrograms(value)) => value.is_positive(),
            Self::Volume(value) => value.is_positive(),
        }
    }

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
}

impl std::ops::Mul<Decimal> for MeasuredQuantity {
    type Output = Self;

    fn mul(self, factor: Decimal) -> Self {
        match self {
            Self::Mass(value) => Self::Mass(value * factor),
            Self::Volume(value) => Self::Volume(value * factor),
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
    pub fn to_millilitres(self) -> Millilitres {
        match self {
            Self::Millilitres(value) => value,
            Self::Litres(value) => Millilitres::new(value.value() * dec!(1000)),
        }
    }
}

impl sealed::Sealed for Volume {}

impl Measurement for Volume {
    fn value(self) -> Decimal {
        Volume::value(self)
    }

    fn unit_symbol(self) -> Cow<'static, str> {
        match self {
            Self::Millilitres(value) => value.unit_symbol(),
            Self::Litres(value) => value.unit_symbol(),
        }
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
}

impl std::ops::Mul<Decimal> for Volume {
    type Output = Self;

    fn mul(self, factor: Decimal) -> Self {
        match self {
            Self::Millilitres(value) => Self::Millilitres(value * factor),
            Self::Litres(value) => Self::Litres(value * factor),
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

pub(crate) type KcalPerKg = Ratio<Kcal, Kilograms>;
pub(crate) type KcalPerCm = Ratio<Kcal, Centimetres>;

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
    let point = i32::try_from(integer.len()).ok()? + exponent;
    if point <= 0 {
        let zeros = usize::try_from(-point).ok()?;
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
