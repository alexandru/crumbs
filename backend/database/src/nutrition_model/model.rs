use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::quantity::{Energy, Grams, Kcal, Mass, MeasuredQuantity, QuantityError, Reported};

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
pub struct Nutrients<T = Option<Reported<Mass>>> {
    #[serde(default)]
    pub fat: Fat<T>,
    #[serde(default)]
    pub carbohydrates: Carbohydrates<T>,
    pub fibre: T,
    pub protein: T,
    pub sodium: T,
    pub salt: T,
    pub cholesterol: T,
    pub water: T,
    pub caffeine: T,
    pub choline: T,
    #[serde(default)]
    pub minerals: Minerals<T>,
    #[serde(default)]
    pub vitamins: Vitamins<T>,
}

impl std::ops::AddAssign<&Nutrients> for Nutrients<Grams> {
    fn add_assign(&mut self, other: &Nutrients) {
        let add = |total: &mut Grams, amount: Option<Reported<Mass>>| {
            if let Some(amount) = amount {
                *total = *total + amount.amount().to_grams();
            }
        };

        add(&mut self.fat.total, other.fat.total);
        add(&mut self.fat.saturated, other.fat.saturated);
        add(&mut self.fat.monounsaturated, other.fat.monounsaturated);
        add(&mut self.fat.polyunsaturated, other.fat.polyunsaturated);
        add(&mut self.fat.trans, other.fat.trans);
        add(
            &mut self.carbohydrates.including_fibre,
            other.carbohydrates.including_fibre,
        );
        add(
            &mut self.carbohydrates.excluding_fibre,
            other.carbohydrates.excluding_fibre,
        );
        add(
            &mut self.carbohydrates.sugars.total,
            other.carbohydrates.sugars.total,
        );
        add(
            &mut self.carbohydrates.sugars.added,
            other.carbohydrates.sugars.added,
        );
        add(
            &mut self.carbohydrates.sugars.lactose,
            other.carbohydrates.sugars.lactose,
        );
        add(&mut self.carbohydrates.starch, other.carbohydrates.starch);
        add(&mut self.fibre, other.fibre);
        add(&mut self.protein, other.protein);
        add(&mut self.sodium, other.sodium);
        add(&mut self.salt, other.salt);
        add(&mut self.cholesterol, other.cholesterol);
        add(&mut self.water, other.water);
        add(&mut self.caffeine, other.caffeine);
        add(&mut self.choline, other.choline);
        add(&mut self.minerals.calcium, other.minerals.calcium);
        add(&mut self.minerals.iron, other.minerals.iron);
        add(&mut self.minerals.magnesium, other.minerals.magnesium);
        add(&mut self.minerals.phosphorus, other.minerals.phosphorus);
        add(&mut self.minerals.potassium, other.minerals.potassium);
        add(&mut self.minerals.zinc, other.minerals.zinc);
        add(&mut self.minerals.copper, other.minerals.copper);
        add(&mut self.minerals.manganese, other.minerals.manganese);
        add(&mut self.minerals.selenium, other.minerals.selenium);
        add(&mut self.vitamins.vitamin_a, other.vitamins.vitamin_a);
        add(&mut self.vitamins.vitamin_c, other.vitamins.vitamin_c);
        add(&mut self.vitamins.vitamin_d, other.vitamins.vitamin_d);
        add(&mut self.vitamins.vitamin_e, other.vitamins.vitamin_e);
        add(&mut self.vitamins.vitamin_k1, other.vitamins.vitamin_k1);
        add(&mut self.vitamins.vitamin_b1, other.vitamins.vitamin_b1);
        add(&mut self.vitamins.vitamin_b2, other.vitamins.vitamin_b2);
        add(&mut self.vitamins.vitamin_b3, other.vitamins.vitamin_b3);
        add(&mut self.vitamins.vitamin_b5, other.vitamins.vitamin_b5);
        add(&mut self.vitamins.vitamin_b6, other.vitamins.vitamin_b6);
        add(&mut self.vitamins.vitamin_b9, other.vitamins.vitamin_b9);
        add(&mut self.vitamins.vitamin_b12, other.vitamins.vitamin_b12);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Minerals<T = Option<Reported<Mass>>> {
    pub calcium: T,
    pub iron: T,
    pub magnesium: T,
    pub phosphorus: T,
    pub potassium: T,
    pub zinc: T,
    pub copper: T,
    pub manganese: T,
    pub selenium: T,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vitamins<T = Option<Reported<Mass>>> {
    pub vitamin_a: T,
    pub vitamin_c: T,
    pub vitamin_d: T,
    pub vitamin_e: T,
    pub vitamin_k1: T,
    pub vitamin_b1: T,
    pub vitamin_b2: T,
    pub vitamin_b3: T,
    pub vitamin_b5: T,
    pub vitamin_b6: T,
    pub vitamin_b9: T,
    pub vitamin_b12: T,
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
pub struct Fat<T = Option<Reported<Mass>>> {
    pub total: T,
    pub saturated: T,
    pub monounsaturated: T,
    pub polyunsaturated: T,
    pub trans: T,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carbohydrates<T = Option<Reported<Mass>>> {
    pub including_fibre: T,
    pub excluding_fibre: T,
    #[serde(default)]
    pub sugars: Sugars<T>,
    pub starch: T,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sugars<T = Option<Reported<Mass>>> {
    pub total: T,
    pub added: T,
    pub lactose: T,
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
