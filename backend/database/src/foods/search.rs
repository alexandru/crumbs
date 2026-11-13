//! Contract for searching indexed foods.

use std::error::Error as StdError;
use std::fmt;
use std::future::Future;

use serde::Serialize;

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

crate::text::string_newtype! {
    /// A nonblank Crumbs full-text expression.
    pub struct SearchQuery;
    /// Creates a search query from a Crumbs full-text expression.
    ///
    /// # Errors
    ///
    /// Returns [`SearchQueryError`] when the expression is blank or contains a
    /// NUL character. Expressions rejected by the current search adapter are reported by
    /// [`FoodSearch::search_foods`](super::FoodSearch::search_foods).
    fn new -> SearchQueryError;
    validate = |value: &str| crate::text::nonblank_without_nul(value).map_err(|error| match error {
        crate::text::TextError::Blank => SearchQueryError::Empty,
        crate::text::TextError::Nul => SearchQueryError::Nul,
    });
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
