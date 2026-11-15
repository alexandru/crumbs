use std::error::Error as StdError;
use std::future::Future;

use serde::{Deserialize, Serialize};

use super::model::{FoodCategory, FoodId, FoodLimit, FoodNotes, StoredFood};
use crate::nutrition_model::FoodData;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct CustomFoodDescription(String);

impl CustomFoodDescription {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidCustomFoodDescription> {
        let value = value.into();
        if value.trim().is_empty() || value.contains('\0') {
            return Err(InvalidCustomFoodDescription);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl TryFrom<String> for CustomFoodDescription {
    type Error = InvalidCustomFoodDescription;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("custom food description must not be blank or contain a NUL character")]
pub struct InvalidCustomFoodDescription;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomFoodInput {
    pub description: CustomFoodDescription,
    #[serde(default)]
    pub category: Option<FoodCategory>,
    #[serde(default)]
    pub notes: Option<FoodNotes>,
    #[serde(default)]
    pub parsed_data: FoodData,
}

pub trait CustomFoodStore: Sync {
    type Error: StdError + Send + Sync + 'static;

    fn create_custom_food(
        &self,
        input: &CustomFoodInput,
    ) -> impl Future<Output = Result<StoredFood, Self::Error>> + Send;

    fn custom_food(
        &self,
        id: FoodId,
    ) -> impl Future<Output = Result<Option<StoredFood>, Self::Error>> + Send;

    fn list_custom_foods(
        &self,
        limit: FoodLimit,
    ) -> impl Future<Output = Result<Vec<StoredFood>, Self::Error>> + Send;

    fn update_custom_food(
        &self,
        id: FoodId,
        input: &CustomFoodInput,
    ) -> impl Future<Output = Result<Option<StoredFood>, Self::Error>> + Send;

    fn delete_custom_food(
        &self,
        id: FoodId,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;
}

#[cfg(test)]
#[path = "../../unit-tests/foods/custom_foods.rs"]
mod tests;
