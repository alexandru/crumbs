//! Source-independent food details stored in `parsed_data`.

use serde::{Deserialize, Serialize};

use crate::measurements::{Energy, Mass, Reported, ServingQuantity, SignedMass};

/// Parsed food details; absence of nutrition does not imply zero nutrients.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoodData {
    pub nutrition: Option<NutritionProfile>,
    #[serde(default)]
    pub serving_sizes: Vec<ServingSize>,
}

/// The standardized measured quantity to which all profile amounts apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NutritionBasis {
    #[serde(rename = "100 g")]
    Per100Grams,
    #[serde(rename = "100 ml")]
    Per100Millilitres,
}

/// Supplied nutrient amounts for one mass or volume basis.
///
/// Fields are independent: totals and components need not sum exactly. Values
/// retain source units and qualifiers; no converted duplicates are stored.
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
    /// Creates an empty profile with an established source basis.
    pub fn new(per: NutritionBasis) -> Self {
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

/// Independently reported fat amounts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fat {
    pub total: Option<Reported<Mass>>,
    pub saturated: Option<Reported<Mass>>,
    pub monounsaturated: Option<Reported<Mass>>,
    pub polyunsaturated: Option<Reported<Mass>>,
    pub trans: Option<Reported<Mass>>,
}

/// Carbohydrate definitions retain their supplied meaning regarding fibre.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carbohydrates {
    pub including_fibre: Option<Reported<SignedMass>>,
    pub excluding_fibre: Option<Reported<SignedMass>>,
    #[serde(default)]
    pub sugars: Sugars,
    pub starch: Option<Reported<Mass>>,
}

/// Supplied sugars; added sugars and lactose can overlap.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sugars {
    pub total: Option<Reported<Mass>>,
    pub added: Option<Reported<Mass>>,
    pub lactose: Option<Reported<Mass>>,
}

/// A named measured portion, without redundant calculated nutrition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServingSize {
    pub name: ServingName,
    pub quantity: ServingQuantity,
}

/// Display name of a measured serving.
pub type ServingName = String;

#[cfg(test)]
#[path = "../unit-tests/nutrition.rs"]
mod tests;
