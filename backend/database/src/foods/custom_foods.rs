//! Custom-food input and management contract.

use std::error::Error as StdError;
use std::future::Future;

use serde::{Deserialize, Serialize};

use super::model::{FoodCategory, FoodId, FoodLimit, FoodNotes, StoredFood};
use crate::nutrition_model::FoodData;

crate::text::string_newtype! {
    /// A nonblank custom food description without NUL characters.
    pub struct CustomFoodDescription;
    /// Creates a custom food description without changing its wording.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidCustomFoodDescription`] if the value is blank after trimming or contains
    /// a NUL character.
    fn new -> InvalidCustomFoodDescription;
    validate = |value: &str| crate::text::nonblank_without_nul(value)
        .map_err(|_| InvalidCustomFoodDescription);
}

impl CustomFoodDescription {
    /// Returns the owned description string.
    pub fn into_inner(self) -> String {
        self.into()
    }
}

/// A custom food description is blank or contains a NUL character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("custom food description must not be blank or contain a NUL character")]
pub struct InvalidCustomFoodDescription;

/// Editable data used to create or replace a custom food.
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

/// Creates and manages foods supplied by Crumbs users.
pub trait CustomFoodStore: Sync {
    /// Failure reported by this storage implementation.
    type Error: StdError + Send + Sync + 'static;

    /// Creates a custom food and returns its stored representation.
    ///
    /// Storage assigns the source, dataset, and source food identifier.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if the food cannot be stored or read back.
    fn create_custom_food(
        &self,
        input: &CustomFoodInput,
    ) -> impl Future<Output = Result<StoredFood, Self::Error>> + Send;

    /// Looks up a custom food by its local Crumbs ID.
    ///
    /// Returns `None` if the ID is missing or does not identify a custom food.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if the food cannot be read or decoded.
    fn custom_food(
        &self,
        id: FoodId,
    ) -> impl Future<Output = Result<Option<StoredFood>, Self::Error>> + Send;

    /// Lists custom foods in local ID order.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if the foods cannot be read or decoded.
    fn list_custom_foods(
        &self,
        limit: FoodLimit,
    ) -> impl Future<Output = Result<Vec<StoredFood>, Self::Error>> + Send;

    /// Replaces a custom food's editable fields while preserving its identity and tags.
    ///
    /// Returns `None` if the ID is missing or does not identify a custom food.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if the food cannot be updated or decoded.
    fn update_custom_food(
        &self,
        id: FoodId,
        input: &CustomFoodInput,
    ) -> impl Future<Output = Result<Option<StoredFood>, Self::Error>> + Send;

    /// Deletes a custom food by its local Crumbs ID.
    ///
    /// Returns `false` if the ID is missing or does not identify a custom food.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if deletion fails.
    fn delete_custom_food(
        &self,
        id: FoodId,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;
}

#[cfg(test)]
#[path = "../../unit-tests/foods/custom_foods.rs"]
mod tests;
