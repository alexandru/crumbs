//! Shared food identities, records, and bounded result counts.

use std::borrow::Cow;
use std::error::Error as StdError;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::nutrition_model::FoodData;

/// Provider identifier; providers may use static or dynamically supplied names.
pub type Source = Cow<'static, str>;
/// Borrowed provider identifier used for exact source-scoped lookups.
pub type SourceRef<'a> = &'a str;
/// Opaque identifier assigned to a food by its source.
pub type SourceFoodId = String;
/// Borrowed opaque identifier used for exact source-scoped lookups.
pub type SourceFoodIdRef<'a> = &'a str;
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

/// A stored food and its source-independent nutritional data.
#[derive(Debug, Serialize)]
pub struct StoredFood {
    /// Local food identity.
    pub id: FoodId,
    /// Food metadata and parsed nutrition.
    #[serde(flatten)]
    pub food: Food,
}

/// A food result count bounded to 20–1,000.
///
/// The default is 20. Converting any signed integer clamps it to this range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "i64", into = "i64")]
pub struct FoodLimit(i64);

impl FoodLimit {
    /// Returns the bounded result count.
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Default for FoodLimit {
    fn default() -> Self {
        Self(20)
    }
}

impl From<i64> for FoodLimit {
    fn from(value: i64) -> Self {
        Self(value.clamp(20, 1_000))
    }
}

impl From<FoodLimit> for i64 {
    fn from(value: FoodLimit) -> Self {
        value.0
    }
}

/// Failure reported while storing or reading food data.
///
/// The underlying cause remains available through [`StdError::source`] without exposing
/// storage-engine types in operation signatures.
#[derive(Debug)]
pub struct FoodStoreError(Box<dyn StdError + Send + Sync + 'static>);

impl fmt::Display for FoodStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.0.as_ref(), formatter)
    }
}

impl StdError for FoodStoreError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.0.as_ref())
    }
}

impl FoodStoreError {
    pub(super) fn new(source: impl StdError + Send + Sync + 'static) -> Self {
        Self(Box::new(source))
    }
}

#[cfg(test)]
#[path = "../../unit-tests/foods/model.rs"]
mod tests;
