//! Full-text search over indexed food descriptions, categories, and tags.

use std::fmt;

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::error::DatabaseError;
use sqlx::sqlite::SqliteError;

use crate::nutrition::FoodData;
use crate::{
    Database, Food, FoodCategory, FoodDescription, FoodId, FoodType, Source, SourceFoodId,
};

/// A nonblank SQLite FTS5 query expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SearchQuery(String);

impl SearchQuery {
    /// Creates a search query from an FTS5 expression.
    ///
    /// # Errors
    ///
    /// Returns [`SearchQueryError`] when the expression is blank or contains a
    /// NUL character. FTS5 syntax errors are reported by [`Database::search_foods`].
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

    /// Returns the FTS5 expression.
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

/// Error produced when an FTS5 expression cannot form a valid [`SearchQuery`].
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

/// Failure while searching indexed food data.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// SQLite rejected the supplied expression as invalid FTS5 syntax.
    #[error("invalid FTS5 search expression: {0}")]
    InvalidQuery(String),
    /// The database could not execute the search or decode stored data.
    #[error("food search failed: {0}")]
    Storage(#[source] sqlx::Error),
}

impl Database {
    /// Searches food descriptions, categories, and tags with an SQLite FTS5 expression.
    ///
    /// Results include full parsed nutrition data. SQLite's default relevance
    /// order is used, with food ID breaking ties.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError::InvalidQuery`] if SQLite rejects the FTS5
    /// expression. Returns [`SearchError::Storage`] if reading or decoding
    /// matching food data fails.
    pub async fn search_foods(
        &self,
        query: &SearchQuery,
        limit: SearchLimit,
    ) -> Result<Vec<FoodSearchResult>, SearchError> {
        let mut connection = self.pool.acquire().await.map_err(SearchError::Storage)?;
        let rows = match sqlx::query_as::<_, FoodSearchRow>(SEARCH_SQL)
            .bind(query.as_str())
            .bind(limit.get())
            .fetch_all(&mut *connection)
            .await
        {
            Ok(rows) => rows,
            Err(error) => {
                let Some(message) = fts_query_error_message(&error).map(str::to_owned) else {
                    return Err(SearchError::Storage(error));
                };
                // SQLite may defer schema errors until step. A valid FTS query
                // separates those errors from diagnostics caused by this input.
                match sqlx::query(SEARCH_SQL)
                    .bind(FTS_ERROR_PROBE_QUERY)
                    .bind(1_i64)
                    .fetch_optional(&mut *connection)
                    .await
                {
                    Ok(_) => return Err(SearchError::InvalidQuery(message)),
                    Err(probe_error) => return Err(SearchError::Storage(probe_error)),
                }
            }
        };

        rows.into_iter().map(FoodSearchRow::into_result).collect()
    }
}

#[derive(FromRow)]
struct FoodSearchRow {
    id: RowId,
    source: SourceText,
    source_food_id: SourceFoodId,
    food_type: FoodTypeText,
    description: FoodDescription,
    category: Option<FoodCategory>,
    parsed_data: ParsedFoodData,
}

type RowId = i64;
type SourceText = String;
type FoodTypeText = String;
type ParsedFoodData = String;

const SEARCH_SQL: &str = "SELECT f.id, f.source, f.source_food_id, f.food_type,
                                  f.description, f.category, f.parsed_data
                           FROM crumbs_foods_fts
                           JOIN crumbs_foods AS f ON f.id = crumbs_foods_fts.rowid
                           WHERE crumbs_foods_fts MATCH ?
                           ORDER BY crumbs_foods_fts.rank ASC, f.id ASC
                           LIMIT ?";

const FTS_ERROR_PROBE_QUERY: &str = "crumbsftsvalidation";

impl FoodSearchRow {
    fn into_result(self) -> Result<FoodSearchResult, SearchError> {
        let parsed_data = serde_json::from_str::<FoodData>(&self.parsed_data)
            .map_err(|error| SearchError::Storage(sqlx::Error::Decode(Box::new(error))))?;
        Ok(FoodSearchResult {
            id: FoodId(self.id),
            food: Food {
                source: Source::Owned(self.source),
                source_food_id: self.source_food_id,
                food_type: FoodType::Owned(self.food_type),
                description: self.description,
                category: self.category,
                parsed_data,
            },
        })
    }
}

fn fts_query_error_message(error: &sqlx::Error) -> Option<&str> {
    let database_error = error.as_database_error()?;
    let sqlite_error = database_error.try_downcast_ref::<SqliteError>()?;
    if is_fts_query_error(sqlite_error.message()) {
        Some(sqlite_error.message())
    } else {
        None
    }
}

fn is_fts_query_error(message: &str) -> bool {
    message == "unterminated string"
        || message.starts_with("fts5: syntax error")
        || message == "fts5: parser stack overflow"
        || message.starts_with("expected integer, got ")
        || message.starts_with("fts5 expression tree is too large")
        || message.starts_with("no such column: ")
}

#[cfg(test)]
#[path = "../unit-tests/search.rs"]
mod tests;
