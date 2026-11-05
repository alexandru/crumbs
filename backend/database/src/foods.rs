//! Source-independent food storage and tag membership.

use std::borrow::Cow;
use std::error::Error as StdError;
use std::future::Future;
use std::hash::{Hash, Hasher};

use serde::Serialize;
use unicase::UniCase;
use unicode_normalization::UnicodeNormalization;

use crate::nutrition::FoodData;

mod search;
mod sqlite;

pub use search::{FoodSearchResult, SearchError, SearchLimit, SearchQuery, SearchQueryError};

/// Persists source-scoped foods independently of their import format.
pub trait FoodWriter: Sync {
    /// Failure reported by this storage implementation.
    type Error: StdError + Send + Sync + 'static;

    /// Atomically inserts or updates a food, preserving its local ID and tags.
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
    /// Results include full parsed nutrition data. Implementations use
    /// best-effort relevance ranking and food ID to break ties.
    /// An expression may contain plain terms (combined as AND), quoted phrases,
    /// prefix matching with a trailing `*` (for example, `chick*`),
    /// `AND`/`OR`/`NOT`, parentheses, and field selectors such as
    /// `source:usda`, `dataset:usda_foundation`, `description:chicken`,
    /// `category:dairy`, or `tags:organic`. USDA datasets include
    /// `usda_sr_legacy` and `usda_survey_fndds`. The default limit is 20, and supplied
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
    ) -> impl Future<Output = Result<Vec<FoodSearchResult>, SearchError<Self::Error>>> + Send;
}

/// Provider identifier; providers may use static or dynamically supplied names.
pub type Source = Cow<'static, str>;
/// Opaque identifier assigned to a food by its source.
pub type SourceFoodId = String;
/// Identifier of a named collection of foods supplied by a source.
pub type Dataset = Cow<'static, str>;
/// Display description supplied by a source.
pub type FoodDescription = String;
/// Optional display category supplied by a source.
pub type FoodCategory = String;

/// A complete food entry supplied by any source.
#[derive(Debug, Serialize)]
pub struct Food {
    pub source: Source,
    pub source_food_id: SourceFoodId,
    pub dataset: Dataset,
    pub description: FoodDescription,
    pub category: Option<FoodCategory>,
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
