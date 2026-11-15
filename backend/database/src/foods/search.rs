use std::error::Error as StdError;
use std::fmt;
use std::future::Future;

use serde::{Deserialize, Serialize};

use super::model::{FoodLimit, StoredFood};

pub trait FoodSearch: Sync {
    type Error: StdError + Send + Sync + 'static;

    fn search_foods(
        &self,
        query: &SearchQuery,
        limit: FoodLimit,
    ) -> impl Future<Output = Result<SearchResults, SearchError<Self::Error>>> + Send;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct SearchQuery(String);

impl SearchQuery {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SearchQueryError {
    #[error("search query must not be empty")]
    Empty,
    #[error("search query must not contain a NUL character")]
    Nul,
}

impl fmt::Display for SearchQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Default, Serialize)]
pub struct SearchResults {
    pub common: Vec<StoredFood>,
    pub branded: Vec<StoredFood>,
    pub custom: Vec<StoredFood>,
}

#[derive(Debug, thiserror::Error)]
pub enum SearchError<E: StdError> {
    #[error("invalid search expression: {0}")]
    InvalidQuery(String),
    #[error("food search failed: {0}")]
    Storage(#[source] E),
}

#[cfg(test)]
#[path = "../../unit-tests/foods/search.rs"]
mod tests;
