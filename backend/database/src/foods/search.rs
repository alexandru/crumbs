//! Contract for searching indexed foods.

use std::error::Error as StdError;
use std::fmt;
use std::future::Future;

use serde::{Deserialize, Serialize};

use super::model::{FoodLimit, StoredFood};

/// Searches source-independent food data.
pub trait FoodSearch: Sync {
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
        limit: FoodLimit,
    ) -> impl Future<Output = Result<SearchResults, SearchError<Self::Error>>> + Send;
}

/// A nonblank Crumbs full-text expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct SearchQuery(String);

impl SearchQuery {
    /// Creates a search query from a Crumbs full-text expression.
    ///
    /// # Errors
    ///
    /// Returns [`SearchQueryError`] when the expression is blank or contains a
    /// NUL character. Expressions rejected by the current search adapter are reported by
    /// [`FoodSearch::search_foods`](super::FoodSearch::search_foods).
    pub fn new(value: impl Into<String>) -> Result<Self, SearchQueryError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SearchQueryError::Empty);
        }
        if value.contains('\0') {
            return Err(SearchQueryError::Nul);
        }
        Ok(Self(value))
    }

    /// Returns the Crumbs full-text expression.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SearchQuery {
    type Error = SearchQueryError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Error produced when input cannot form a valid [`SearchQuery`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SearchQueryError {
    /// The expression contains only whitespace.
    #[error("search query must not be empty")]
    Empty,
    /// The expression contains a NUL character.
    #[error("search query must not contain a NUL character")]
    Nul,
}

impl fmt::Display for SearchQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Search results grouped by the dataset assigned to each food.
#[derive(Debug, Default, Serialize)]
pub struct SearchResults {
    pub common: Vec<StoredFood>,
    pub branded: Vec<StoredFood>,
    pub custom: Vec<StoredFood>,
}

/// Failure while searching indexed food data.
#[derive(Debug, thiserror::Error)]
pub enum SearchError<E: StdError> {
    /// The search adapter rejected the supplied expression.
    #[error("invalid search expression: {0}")]
    InvalidQuery(String),
    /// The data store could not execute the search or decode stored data.
    #[error("food search failed: {0}")]
    Storage(#[source] E),
}

#[cfg(test)]
#[path = "../../unit-tests/foods/search.rs"]
mod tests;
