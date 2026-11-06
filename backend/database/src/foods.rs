//! Source-independent food storage and tag membership.

use std::borrow::Cow;
use std::error::Error as StdError;
use std::fmt;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use unicase::UniCase;
use unicode_normalization::UnicodeNormalization;

use crate::nutrition::FoodData;

mod search;
mod sqlite;

pub use search::{
    FoodSearchResult, FoodSearchResults, SearchError, SearchLimit, SearchQuery, SearchQueryError,
};

/// Persists source-scoped foods independently of their import format.
pub trait FoodWriter: Sync {
    /// Failure reported by this storage implementation.
    type Error: StdError + Send + Sync + 'static;

    /// Atomically inserts a complete source-supplied food or replaces its source-supplied fields,
    /// preserving its local ID and tags.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if the food cannot be persisted.
    fn upsert_food(&self, food: &Food) -> impl Future<Output = Result<FoodId, Self::Error>> + Send;
}

/// Searches source-independent food data.
pub trait FoodReader: Sync {
    /// Failure reported by this storage implementation.
    type Error: StdError + Send + Sync + 'static;

    /// Searches indexed sources, datasets, descriptions, categories, and tags with a Crumbs
    /// full-text expression.
    ///
    /// Results include full parsed nutrition data and are grouped by dataset. The limit applies
    /// independently to each dataset. Implementations use best-effort relevance ranking and food
    /// ID to break ties within each dataset.
    /// An expression may contain plain terms (combined as AND), quoted phrases,
    /// prefix matching with a trailing `*` (for example, `chick*`),
    /// `AND`/`OR`/`NOT`, parentheses, and field selectors such as
    /// `source:usda`, `dataset:common`, `description:chicken`, `category:dairy`, or
    /// `tags:organic`. The default limit is 20 per dataset, and supplied
    /// limits are clamped to 20–1,000.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError::InvalidQuery`] if the search adapter rejects the
    /// expression. Returns [`SearchError::Storage`] if the data store cannot
    /// search or decode matching data.
    fn search_foods(
        &self,
        query: &SearchQuery,
        limit: SearchLimit,
    ) -> impl Future<Output = Result<FoodSearchResults, SearchError<Self::Error>>> + Send;
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
    ) -> impl Future<Output = Result<FoodSearchResult, Self::Error>> + Send;

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
    ) -> impl Future<Output = Result<Option<FoodSearchResult>, Self::Error>> + Send;

    /// Lists custom foods in local ID order.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if the foods cannot be read or decoded.
    fn list_custom_foods(
        &self,
        limit: SearchLimit,
    ) -> impl Future<Output = Result<Vec<FoodSearchResult>, Self::Error>> + Send;

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
    ) -> impl Future<Output = Result<Option<FoodSearchResult>, Self::Error>> + Send;

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

/// Provider identifier; providers may use static or dynamically supplied names.
pub type Source = Cow<'static, str>;
/// Opaque identifier assigned to a food by its source.
pub type SourceFoodId = String;
/// A search group assigned to each food.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dataset {
    /// Foods intended for general, non-branded ingredient search.
    Common,
    /// Branded food products.
    Branded,
    /// Foods created and managed in Crumbs.
    Custom,
}

impl Dataset {
    /// Returns the storage and wire identifier for this dataset.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Branded => "branded",
            Self::Custom => "custom",
        }
    }
}

impl fmt::Display for Dataset {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Dataset {
    type Err = InvalidDataset;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "common" => Ok(Self::Common),
            "branded" => Ok(Self::Branded),
            "custom" => Ok(Self::Custom),
            _ => Err(InvalidDataset(value.to_owned())),
        }
    }
}

impl From<Dataset> for String {
    fn from(value: Dataset) -> Self {
        value.as_str().to_owned()
    }
}

/// An unrecognized dataset identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid dataset identifier: {0}")]
pub struct InvalidDataset(String);

impl InvalidDataset {
    /// Returns the rejected dataset identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Display description supplied by a source.
pub type FoodDescription = String;
/// Optional display category supplied by a source.
pub type FoodCategory = String;
/// Optional notes stored with a food and excluded from search indexing.
pub type FoodNotes = String;

/// A nonblank custom food description without NUL characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CustomFoodDescription(String);

impl CustomFoodDescription {
    /// Creates a custom food description without changing its wording.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidCustomFoodDescription`] if the value is blank after trimming or contains
    /// a NUL character.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidCustomFoodDescription> {
        let value = value.into();
        if value.trim().is_empty() || value.contains('\0') {
            return Err(InvalidCustomFoodDescription);
        }
        Ok(Self(value))
    }

    /// Returns the description as supplied.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the owned description string.
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

impl From<CustomFoodDescription> for String {
    fn from(value: CustomFoodDescription) -> Self {
        value.0
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

/// A complete food entry supplied by any source.
#[derive(Debug, Serialize)]
pub struct Food {
    pub source: Source,
    pub source_food_id: SourceFoodId,
    pub dataset: Dataset,
    pub description: FoodDescription,
    pub category: Option<FoodCategory>,
    pub notes: Option<FoodNotes>,
    /// Shared Crumbs model parsed from the source record.
    pub parsed_data: FoodData,
}

/// Local food identity, independent of source identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct FoodId(pub i64);

/// Food metadata returned by source-identity lookup, without loading the source document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoodSummary {
    pub id: FoodId,
    pub source: Source,
    pub source_food_id: SourceFoodId,
    pub dataset: Dataset,
    pub description: FoodDescription,
    pub category: Option<FoodCategory>,
    pub notes: Option<FoodNotes>,
}

/// Local identity of a reusable tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagId(pub i64);

/// A reusable tag, retaining the spelling used when it was first created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id: TagId,
    pub name: TagName,
}

/// A nonblank tag name with surrounding whitespace removed.
///
/// Equality uses Unicode canonical caseless matching and preserves accents.
#[derive(Debug, Clone)]
pub struct TagName {
    display: TagDisplayName,
    key: TagKey,
}

impl TagName {
    /// Constructs a tag name.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTagName`] if trimming leaves no characters.
    pub fn new(value: impl AsRef<str>) -> Result<Self, InvalidTagName> {
        let display = value.as_ref().trim();
        if display.is_empty() {
            return Err(InvalidTagName);
        }
        let decomposed = display.nfd().collect::<String>();
        let key = UniCase::new(decomposed).to_folded_case().nfd().collect();
        Ok(Self {
            display: display.to_owned(),
            key,
        })
    }

    /// Returns the display spelling.
    pub fn as_str(&self) -> &str {
        &self.display
    }
}

impl PartialEq for TagName {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for TagName {}

impl Hash for TagName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

/// Tag names cannot be blank.
#[derive(Debug, thiserror::Error)]
#[error("tag name must not be blank")]
pub struct InvalidTagName;

type TagDisplayName = String;
type TagKey = String;

#[cfg(test)]
#[path = "../unit-tests/foods.rs"]
mod tests;
