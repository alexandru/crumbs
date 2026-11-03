//! Source-independent parsed food data: the shared model for imported foods.
//!
//! [`ParsedFoodData`] is what importers persist as the `parsed_data` JSON
//! column instead of a provider document. Source adapters translate their
//! records into this model, so consumers read nutrition without knowing any
//! provider schema. The model must stay extensible: nutrients outside the
//! initial coverage are simply absent, not repurposed.

use std::fmt;

use serde::Deserialize;
use serde::Serialize;
use serde::de::Visitor;

use crate::measure::{Energy, Mass, MeasureError, MeasuredQuantity};

/// Nutrition and serving definitions parsed from one source record.
///
/// A food may lack nutrition data and still have serving definitions, and the
/// reverse. Serialized as the JSON payload of the `parsed_data` column.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedFoodData {
    /// Nutrition profile for its basis, or `null` when nothing is known.
    pub nutrition: Option<NutritionProfile>,
    /// Named serving definitions with measured quantities.
    pub serving_sizes: Vec<ServingSize>,
}

/// Reported nutrients for a fixed basis of 100 grams or 100 millilitres.
///
/// Every amount is optional: a missing amount is `null`, never a zero default.
/// Reported zero is a valid value. Carbohydrate meanings stay explicit, and no
/// amount is derived from another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NutritionProfile {
    /// The measured quantity the amounts apply to.
    pub per: NutritionBasis,
    /// One energy amount in its selected source unit.
    pub energy: Option<Energy>,
    /// Fat amounts.
    pub fat: Fat,
    /// Carbohydrate amounts with their fibre meanings kept explicit.
    pub carbohydrates: Carbohydrates,
    /// Dietary fibre.
    pub fibre: Option<Mass>,
    /// Protein.
    pub protein: Option<Mass>,
    /// Sodium.
    pub sodium: Option<Mass>,
    /// Salt.
    pub salt: Option<Mass>,
    /// Cholesterol.
    pub cholesterol: Option<Mass>,
}

/// The measured quantity a nutrition profile applies to: 100 g or 100 ml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NutritionBasis {
    /// Per 100 grams, serialized as `"100 g"`.
    Per100Grams,
    /// Per 100 millilitres, serialized as `"100 ml"`.
    Per100Millilitres,
}

impl NutritionBasis {
    /// Parses a basis from its unit-bearing form.
    ///
    /// # Errors
    ///
    /// Returns [`MeasureError`] for any other string.
    pub fn parse(text: &str) -> Result<Self, MeasureError> {
        match text.trim() {
            "100 g" => Ok(Self::Per100Grams),
            "100 ml" => Ok(Self::Per100Millilitres),
            other => Err(MeasureError::Malformed {
                text: other.to_owned(),
            }),
        }
    }
}

impl fmt::Display for NutritionBasis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Per100Grams => "100 g",
            Self::Per100Millilitres => "100 ml",
        })
    }
}

impl Serialize for NutritionBasis {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for NutritionBasis {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(BasisVisitor)
    }
}

/// Fat amounts for a nutrition profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fat {
    /// Total fat.
    pub total: Option<Mass>,
    /// Saturated fat.
    pub saturated: Option<Mass>,
    /// Monounsaturated fat.
    pub monounsaturated: Option<Mass>,
    /// Polyunsaturated fat.
    pub polyunsaturated: Option<Mass>,
    /// Trans fat.
    pub trans: Option<Mass>,
}

/// Carbohydrate amounts, keeping the fibre meaning of each value explicit.
///
/// Total carbohydrate including fibre and carbohydrate excluding fibre are
/// separately supplied values; one is never relabelled as the other, and no
/// subtraction is stored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carbohydrates {
    /// Total carbohydrate including fibre.
    pub including_fibre: Option<Mass>,
    /// Carbohydrate excluding fibre.
    pub excluding_fibre: Option<Mass>,
    /// Sugar amounts; added sugars and lactose may overlap with the total.
    pub sugars: Sugars,
    /// Starch.
    pub starch: Option<Mass>,
}

/// Sugar amounts for a nutrition profile.
///
/// Total sugars, added sugars, and lactose are independently supplied; added
/// sugars and lactose may overlap and are not summed as disjoint components.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sugars {
    /// Total sugars.
    pub total: Option<Mass>,
    /// Added sugars.
    pub added: Option<Mass>,
    /// Lactose.
    pub lactose: Option<Mass>,
}

/// A named serving of a food with its measured mass or volume.
///
/// Serving definitions store no nutrition of their own; a serving's nutrition
/// is derived when needed from the profile and this quantity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServingSize {
    /// The serving's nonblank name.
    pub name: ServingName,
    /// The serving's positive measured mass or volume.
    pub quantity: MeasuredQuantity,
}

/// A nonblank serving name with surrounding whitespace removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServingName(String);

impl ServingName {
    /// Constructs a serving name, trimming surrounding whitespace.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidServingName`] if trimming leaves no characters.
    pub fn new(value: impl AsRef<str>) -> Result<Self, InvalidServingName> {
        let trimmed = value.as_ref().trim();
        if trimmed.is_empty() {
            return Err(InvalidServingName);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// Returns the name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServingName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ServingName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ServingName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(ServingNameVisitor)
    }
}

/// Serving names cannot be blank.
#[derive(Debug, thiserror::Error)]
#[error("serving name must not be blank")]
pub struct InvalidServingName;

/// Deserializes a nutrition basis from its unit-bearing form.
struct BasisVisitor;

impl<'de> Visitor<'de> for BasisVisitor {
    type Value = NutritionBasis;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a nutrition basis, `100 g` or `100 ml`")
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<NutritionBasis, E> {
        NutritionBasis::parse(value).map_err(serde::de::Error::custom)
    }
}

/// Deserializes a serving name, rejecting blank names.
struct ServingNameVisitor;

impl<'de> Visitor<'de> for ServingNameVisitor {
    type Value = ServingName;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a nonblank serving name")
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<ServingName, E> {
        ServingName::new(value).map_err(serde::de::Error::custom)
    }
}

/// Reports whether a profile carries at least one nutrient amount.
pub(crate) fn reports_any_amount(profile: &NutritionProfile) -> bool {
    profile.energy.is_some()
        || profile.fat.total.is_some()
        || profile.fat.saturated.is_some()
        || profile.fat.monounsaturated.is_some()
        || profile.fat.polyunsaturated.is_some()
        || profile.fat.trans.is_some()
        || profile.carbohydrates.including_fibre.is_some()
        || profile.carbohydrates.excluding_fibre.is_some()
        || profile.carbohydrates.sugars.total.is_some()
        || profile.carbohydrates.sugars.added.is_some()
        || profile.carbohydrates.sugars.lactose.is_some()
        || profile.carbohydrates.starch.is_some()
        || profile.fibre.is_some()
        || profile.protein.is_some()
        || profile.sodium.is_some()
        || profile.salt.is_some()
        || profile.cholesterol.is_some()
}

#[cfg(test)]
#[path = "../unit-tests/parsed.rs"]
mod tests;
