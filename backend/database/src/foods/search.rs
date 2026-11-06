//! Public contract for searching indexed foods.

use std::error::Error as StdError;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::{Food, FoodId};

/// A nonblank Crumbs full-text expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SearchQuery(String);

impl SearchQuery {
    /// Creates a search query from a Crumbs full-text expression.
    ///
    /// # Errors
    ///
    /// Returns [`SearchQueryError`] when the expression is blank or contains a
    /// NUL character. Expressions rejected by the current search adapter are reported by
    /// [`FoodReader::search_foods`](super::FoodReader::search_foods).
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

impl From<SearchQuery> for String {
    fn from(value: SearchQuery) -> Self {
        value.0
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

/// A food result count bounded to 20–1,000.
///
/// The default is 20. Converting any signed integer clamps it to this range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "i64", into = "i64")]
pub struct SearchLimit(i64);

impl SearchLimit {
    /// Returns the bounded result count.
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Default for SearchLimit {
    fn default() -> Self {
        Self(20)
    }
}

impl From<i64> for SearchLimit {
    fn from(value: i64) -> Self {
        Self(value.clamp(20, 1_000))
    }
}

impl From<SearchLimit> for i64 {
    fn from(value: SearchLimit) -> Self {
        value.0
    }
}

impl fmt::Display for SearchQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A matching food and its source-independent nutritional data.
#[derive(Debug, Serialize)]
pub struct FoodSearchResult {
    /// Local food identity.
    pub id: FoodId,
    /// Matching food metadata and parsed nutrition.
    #[serde(flatten)]
    pub food: Food,
}

/// Search results grouped by the dataset assigned to each food.
#[derive(Debug, Default, Serialize)]
pub struct FoodSearchResults {
    pub common: Vec<FoodSearchResult>,
    pub branded: Vec<FoodSearchResult>,
    pub custom: Vec<FoodSearchResult>,
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
