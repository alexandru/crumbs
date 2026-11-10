//! Source-independent nutrition and measured serving definitions.

use serde::{Deserialize, Serialize};

use super::quantity::{Energy, Mass, MeasuredQuantity, QuantityError, Reported};

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

impl NutritionProfile {
    /// Creates a profile with no reported nutrients on the selected basis.
    pub fn empty(per: NutritionBasis) -> Self {
        Self {
            per,
            energy: None,
            fat: Fat::default(),
            carbohydrates: Carbohydrates::default(),
            fibre: None,
            protein: None,
            sodium: None,
            salt: None,
            cholesterol: None,
        }
    }
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

#[cfg(test)]
#[path = "../../unit-tests/nutrition_model/model.rs"]
mod tests;
