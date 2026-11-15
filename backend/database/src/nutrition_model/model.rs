use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::quantity::{Energy, Kcal, Mass, MeasuredQuantity, QuantityError, Reported};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoodData {
    pub nutrition: Option<NutritionProfile>,
    #[serde(default)]
    pub serving_sizes: Vec<ServingSize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NutritionBasis {
    #[serde(rename = "100 g")]
    Per100Grams,
    #[serde(rename = "100 ml")]
    Per100Millilitres,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NutritionProfile {
    pub per: NutritionBasis,
    pub energy: Option<Reported<Energy>>,
    #[serde(flatten)]
    pub nutrients: Nutrients,
}

impl NutritionProfile {
    pub fn empty(per: NutritionBasis) -> Self {
        Self {
            per,
            energy: None,
            nutrients: Nutrients::default(),
        }
    }
}

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
    pub choline: Option<Reported<Mass>>,
    #[serde(default)]
    pub minerals: Minerals,
    #[serde(default)]
    pub vitamins: Vitamins,
}

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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vitamins {
    pub vitamin_a: Option<Reported<Mass>>,
    pub vitamin_c: Option<Reported<Mass>>,
    pub vitamin_d: Option<Reported<Mass>>,
    pub vitamin_e: Option<Reported<Mass>>,
    pub vitamin_k1: Option<Reported<Mass>>,
    pub vitamin_b1: Option<Reported<Mass>>,
    pub vitamin_b2: Option<Reported<Mass>>,
    pub vitamin_b3: Option<Reported<Mass>>,
    pub vitamin_b5: Option<Reported<Mass>>,
    pub vitamin_b6: Option<Reported<Mass>>,
    pub vitamin_b9: Option<Reported<Mass>>,
    pub vitamin_b12: Option<Reported<Mass>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumedNutrition {
    pub calories: Reported<Kcal>,
    #[serde(flatten)]
    pub nutrients: Nutrients,
}

impl ConsumedNutrition {
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fat {
    pub total: Option<Reported<Mass>>,
    pub saturated: Option<Reported<Mass>>,
    pub monounsaturated: Option<Reported<Mass>>,
    pub polyunsaturated: Option<Reported<Mass>>,
    pub trans: Option<Reported<Mass>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carbohydrates {
    pub including_fibre: Option<Reported<Mass>>,
    pub excluding_fibre: Option<Reported<Mass>>,
    #[serde(default)]
    pub sugars: Sugars,
    pub starch: Option<Reported<Mass>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sugars {
    pub total: Option<Reported<Mass>>,
    pub added: Option<Reported<Mass>>,
    pub lactose: Option<Reported<Mass>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServingSize {
    pub name: ServingName,
    quantity: MeasuredQuantity,
}

impl ServingSize {
    pub fn new(name: ServingName, quantity: MeasuredQuantity) -> Result<Self, QuantityError> {
        if !quantity.is_positive() {
            return Err(QuantityError::NonPositive);
        }
        Ok(Self { name, quantity })
    }

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

pub type ServingName = String;

#[cfg(test)]
#[path = "../../unit-tests/nutrition_model/model.rs"]
mod tests;
