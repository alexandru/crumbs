//! Source-independent food models and storage contracts.

mod custom_foods;
mod model;
mod search;
mod source_foods;
mod sqlite;
mod tags;

pub use custom_foods::{
    CustomFoodDescription, CustomFoodInput, CustomFoodStore, InvalidCustomFoodDescription,
};
pub use model::{
    Dataset, Food, FoodCategory, FoodDescription, FoodId, FoodLimit, FoodNotes, FoodStoreError,
    FoodSummary, InvalidDataset, Source, SourceFoodId, SourceFoodIdRef, SourceRef, StoredFood,
};
pub use search::{FoodSearch, SearchError, SearchQuery, SearchQueryError, SearchResults};
pub use source_foods::{FoodLookup, FoodWriter};
pub use tags::{InvalidTagName, Tag, TagId, TagName, TagStore};

#[cfg(test)]
#[path = "../../unit-tests/foods/mod.rs"]
mod tests;
