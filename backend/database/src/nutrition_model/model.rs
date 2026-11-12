//! Source-independent nutrition and measured serving definitions.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::quantity::{Energy, Kcal, Mass, MeasuredQuantity, QuantityError, Reported};

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
    pub water: Option<Reported<Mass>>,
    pub caffeine: Option<Reported<Mass>>,
    /// Total choline.
    pub choline: Option<Reported<Mass>>,
    #[serde(default)]
    pub minerals: Minerals,
    #[serde(default)]
    pub vitamins: Vitamins,
}

/// Reported mineral amounts. Missing amounts remain unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Minerals {
    pub calcium: Option<Reported<Mass>>,
    pub iron: Option<Reported<Mass>>,
    pub magnesium: Option<Reported<Mass>>,
    pub phosphorus: Option<Reported<Mass>>,
    pub potassium: Option<Reported<Mass>>,
    pub zinc: Option<Reported<Mass>>,
    pub copper: Option<Reported<Mass>>,
    pub manganese: Option<Reported<Mass>>,
    pub selenium: Option<Reported<Mass>>,
}

/// Reported vitamin amounts in the specified forms, with source mass units.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vitamins {
    /// Vitamin A as retinol activity equivalents.
    pub vitamin_a: Option<Reported<Mass>>,
    /// Total ascorbic acid.
    pub vitamin_c: Option<Reported<Mass>>,
    /// Total vitamin D2 and D3.
    pub vitamin_d: Option<Reported<Mass>>,
    /// Alpha-tocopherol.
    pub vitamin_e: Option<Reported<Mass>>,
    /// Phylloquinone.
    pub vitamin_k1: Option<Reported<Mass>>,
    /// Thiamin.
    pub vitamin_b1: Option<Reported<Mass>>,
    /// Riboflavin.
    pub vitamin_b2: Option<Reported<Mass>>,
    /// Niacin, without conversion to niacin equivalents.
    pub vitamin_b3: Option<Reported<Mass>>,
    /// Pantothenic acid.
    pub vitamin_b5: Option<Reported<Mass>>,
    /// Total vitamin B6.
    pub vitamin_b6: Option<Reported<Mass>>,
    /// Total folate, without conversion to dietary folate equivalents.
    pub vitamin_b9: Option<Reported<Mass>>,
    /// Vitamin B12.
    pub vitamin_b12: Option<Reported<Mass>>,
}

/// Consumed nutrition totals, separate from source nutrition per 100 units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumedNutrition {
    pub calories: Reported<Kcal>,
    #[serde(flatten)]
    pub nutrients: Nutrients,
}

impl ConsumedNutrition {
    /// Builds consumed totals from source nutrition and a portion factor.
    ///
    /// # Errors
    ///
    /// Returns an error for a negative nutrient amount or a strict upper bound of zero.
    pub fn from_source(
        energy: Reported<Energy>,
        nutrients: &Nutrients,
        factor: Decimal,
    ) -> Result<Self, QuantityError> {
        let nutrient = |value: Option<Reported<Mass>>| {
            value
                .map(|value| Reported::new(value.qualifier(), value.amount() * factor))
                .transpose()
        };

        Ok(Self {
            calories: Reported::new(
                energy.qualifier(),
                (energy.amount() * factor).to_kilocalories(),
            )?,
            nutrients: Nutrients {
                fat: Fat {
                    total: nutrient(nutrients.fat.total)?,
                    saturated: nutrient(nutrients.fat.saturated)?,
                    monounsaturated: nutrient(nutrients.fat.monounsaturated)?,
                    polyunsaturated: nutrient(nutrients.fat.polyunsaturated)?,
                    trans: nutrient(nutrients.fat.trans)?,
                },
                carbohydrates: Carbohydrates {
                    including_fibre: nutrient(nutrients.carbohydrates.including_fibre)?,
                    excluding_fibre: nutrient(nutrients.carbohydrates.excluding_fibre)?,
                    sugars: Sugars {
                        total: nutrient(nutrients.carbohydrates.sugars.total)?,
                        added: nutrient(nutrients.carbohydrates.sugars.added)?,
                        lactose: nutrient(nutrients.carbohydrates.sugars.lactose)?,
                    },
                    starch: nutrient(nutrients.carbohydrates.starch)?,
                },
                fibre: nutrient(nutrients.fibre)?,
                protein: nutrient(nutrients.protein)?,
                sodium: nutrient(nutrients.sodium)?,
                salt: nutrient(nutrients.salt)?,
                cholesterol: nutrient(nutrients.cholesterol)?,
                water: nutrient(nutrients.water)?,
                caffeine: nutrient(nutrients.caffeine)?,
                choline: nutrient(nutrients.choline)?,
                minerals: Minerals {
                    calcium: nutrient(nutrients.minerals.calcium)?,
                    iron: nutrient(nutrients.minerals.iron)?,
                    magnesium: nutrient(nutrients.minerals.magnesium)?,
                    phosphorus: nutrient(nutrients.minerals.phosphorus)?,
                    potassium: nutrient(nutrients.minerals.potassium)?,
                    zinc: nutrient(nutrients.minerals.zinc)?,
                    copper: nutrient(nutrients.minerals.copper)?,
                    manganese: nutrient(nutrients.minerals.manganese)?,
                    selenium: nutrient(nutrients.minerals.selenium)?,
                },
                vitamins: Vitamins {
                    vitamin_a: nutrient(nutrients.vitamins.vitamin_a)?,
                    vitamin_c: nutrient(nutrients.vitamins.vitamin_c)?,
                    vitamin_d: nutrient(nutrients.vitamins.vitamin_d)?,
                    vitamin_e: nutrient(nutrients.vitamins.vitamin_e)?,
                    vitamin_k1: nutrient(nutrients.vitamins.vitamin_k1)?,
                    vitamin_b1: nutrient(nutrients.vitamins.vitamin_b1)?,
                    vitamin_b2: nutrient(nutrients.vitamins.vitamin_b2)?,
                    vitamin_b3: nutrient(nutrients.vitamins.vitamin_b3)?,
                    vitamin_b5: nutrient(nutrients.vitamins.vitamin_b5)?,
                    vitamin_b6: nutrient(nutrients.vitamins.vitamin_b6)?,
                    vitamin_b9: nutrient(nutrients.vitamins.vitamin_b9)?,
                    vitamin_b12: nutrient(nutrients.vitamins.vitamin_b12)?,
                },
            },
        })
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
