//! Source-independent nutrition and measured serving definitions.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::quantity::{Energy, Kcal, Mass, MeasuredQuantity, Measurement, QuantityError, Reported};

/// Imported food details. Missing nutrition does not imply zero nutrients.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoodData {
    pub nutrition: Option<NutritionProfile>,
    #[serde(default)]
    pub serving_sizes: Vec<ServingSize>,
}

/// The standardized measured quantity to which a nutrition profile applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NutritionBasis {
    #[serde(rename = "100 g")]
    Per100Grams,
    #[serde(rename = "100 ml")]
    Per100Millilitres,
}

/// Source-supplied nutrients on one mass or volume basis.
/// Independent reported totals and components need not add up exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NutritionProfile {
    pub per: NutritionBasis,
    pub energy: Option<Reported<Energy>>,
    #[serde(flatten)]
    pub nutrients: Nutrients,
}

impl NutritionProfile {
    /// Creates a profile with no reported nutrients on the selected basis.
    pub fn empty(per: NutritionBasis) -> Self {
        Self {
            per,
            energy: None,
            nutrients: Nutrients::default(),
        }
    }
}

/// Reported non-energy nutrients, preserving source units and qualifiers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nutrients {
    #[serde(default)]
    pub fat: Fat,
    #[serde(default)]
    pub carbohydrates: Carbohydrates,
    pub fibre: Option<Reported<Mass>>,
    pub protein: Option<Reported<Mass>>,
    pub sodium: Option<Reported<Mass>>,
    pub salt: Option<Reported<Mass>>,
    pub cholesterol: Option<Reported<Mass>>,
}

impl Nutrients {
    /// Scales every available nutrient while retaining qualifiers, units, and missing values.
    ///
    /// # Errors
    ///
    /// Returns [`QuantityError::Arithmetic`] if any scaled value is outside the representable
    /// decimal range, [`QuantityError::Negative`] if a result is negative, or
    /// [`QuantityError::ZeroStrictUpperBound`] if a strict upper bound rounds to zero.
    pub fn checked_scale(&self, factor: Decimal) -> Result<Self, QuantityError> {
        Ok(Self {
            fat: self.fat.checked_scale(factor)?,
            carbohydrates: self.carbohydrates.checked_scale(factor)?,
            fibre: checked_scale_optional(self.fibre, factor)?,
            protein: checked_scale_optional(self.protein, factor)?,
            sodium: checked_scale_optional(self.sodium, factor)?,
            salt: checked_scale_optional(self.salt, factor)?,
            cholesterol: checked_scale_optional(self.cholesterol, factor)?,
        })
    }
}

/// Consumed nutrition totals, separate from source nutrition per 100 units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumedNutrition {
    pub calories: Reported<Kcal>,
    #[serde(flatten)]
    pub nutrients: Nutrients,
}

/// Reported fat amounts, retaining each supplied unit and qualifier.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fat {
    pub total: Option<Reported<Mass>>,
    pub saturated: Option<Reported<Mass>>,
    pub monounsaturated: Option<Reported<Mass>>,
    pub polyunsaturated: Option<Reported<Mass>>,
    pub trans: Option<Reported<Mass>>,
}

/// Distinct supplied carbohydrate definitions; fibre is never subtracted here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carbohydrates {
    pub including_fibre: Option<Reported<Mass>>,
    pub excluding_fibre: Option<Reported<Mass>>,
    #[serde(default)]
    pub sugars: Sugars,
    pub starch: Option<Reported<Mass>>,
}

/// Reported sugars. Added sugars and lactose can overlap.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sugars {
    pub total: Option<Reported<Mass>>,
    pub added: Option<Reported<Mass>>,
    pub lactose: Option<Reported<Mass>>,
}

/// A named portion with a known positive mass or volume.
/// Nutrition remains on the profile basis and is not copied into servings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServingSize {
    pub name: ServingName,
    quantity: MeasuredQuantity,
}

impl ServingSize {
    /// Creates a named serving with a positive mass or volume.
    pub fn new(name: ServingName, quantity: MeasuredQuantity) -> Result<Self, QuantityError> {
        if !quantity.is_positive() {
            return Err(QuantityError::NonPositive);
        }
        Ok(Self { name, quantity })
    }

    /// Returns the serving's measured mass or volume.
    pub const fn quantity(&self) -> MeasuredQuantity {
        self.quantity
    }
}

impl<'de> Deserialize<'de> for ServingSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Fields {
            name: ServingName,
            quantity: MeasuredQuantity,
        }

        let fields = Fields::deserialize(deserializer)?;
        Self::new(fields.name, fields.quantity).map_err(serde::de::Error::custom)
    }
}

/// Source-supplied display name of a measured portion.
pub type ServingName = String;

impl Fat {
    fn checked_scale(&self, factor: Decimal) -> Result<Self, QuantityError> {
        Ok(Self {
            total: checked_scale_optional(self.total, factor)?,
            saturated: checked_scale_optional(self.saturated, factor)?,
            monounsaturated: checked_scale_optional(self.monounsaturated, factor)?,
            polyunsaturated: checked_scale_optional(self.polyunsaturated, factor)?,
            trans: checked_scale_optional(self.trans, factor)?,
        })
    }
}

impl Carbohydrates {
    fn checked_scale(&self, factor: Decimal) -> Result<Self, QuantityError> {
        Ok(Self {
            including_fibre: checked_scale_optional(self.including_fibre, factor)?,
            excluding_fibre: checked_scale_optional(self.excluding_fibre, factor)?,
            sugars: self.sugars.checked_scale(factor)?,
            starch: checked_scale_optional(self.starch, factor)?,
        })
    }
}

impl Sugars {
    fn checked_scale(&self, factor: Decimal) -> Result<Self, QuantityError> {
        Ok(Self {
            total: checked_scale_optional(self.total, factor)?,
            added: checked_scale_optional(self.added, factor)?,
            lactose: checked_scale_optional(self.lactose, factor)?,
        })
    }
}

fn checked_scale_optional<T: Measurement>(
    value: Option<Reported<T>>,
    factor: Decimal,
) -> Result<Option<Reported<T>>, QuantityError> {
    value.map(|value| value.checked_scale(factor)).transpose()
}

#[cfg(test)]
#[path = "../../unit-tests/nutrition_model/model.rs"]
mod tests;
