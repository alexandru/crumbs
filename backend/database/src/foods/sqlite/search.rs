//! Full-text index invariants for food metadata and normalized tag names.
//!
//! `crumbs_foods_fts` is an external-content FTS5 table over
//! `crumbs_foods_search_content`, a view projecting source, dataset, food
//! description, category, and ordered tag names from `crumbs_tags` and
//! `crumbs_food_tags`. Source data and display values remain in their normalized
//! tables; FTS stores token terms.
//! `content_rowid` is the view's food `id`.
//!
//! Food triggers reindex changed `id`, `source`, `dataset`, `description`, or
//! `category` values. Assignment and tag triggers reindex documents when tag
//! membership or names change. `IS NOT` comparisons avoid rewriting unchanged
//! metadata. The tokenizer is `unicode61 remove_diacritics 2`: it folds case
//! and Latin diacritics, but it does not stem, translate, or treat different
//! letters such as `â` and `î` as interchangeable.

use sqlx::SqliteConnection;
use sqlx::error::DatabaseError;
use sqlx::sqlite::SqliteError;

use super::rows::StoredFoodRow;
use crate::foods::{
    Dataset, FoodLimit, FoodSearch, FoodStoreError, SearchError, SearchQuery, SearchResults,
    StoredFood,
};
use crate::runtime::Database;

impl FoodSearch for Database {
    type Error = FoodStoreError;

    async fn search_foods(
        &self,
        query: &SearchQuery,
        limit: FoodLimit,
    ) -> Result<SearchResults, SearchError<Self::Error>> {
        let operation: Result<SearchResults, SearchError<sqlx::Error>> = async {
            let mut connection = self.pool.acquire().await.map_err(SearchError::Storage)?;
            Ok(SearchResults {
                common: search_dataset(&mut connection, query, Dataset::Common, limit).await?,
                branded: search_dataset(&mut connection, query, Dataset::Branded, limit).await?,
                custom: search_dataset(&mut connection, query, Dataset::Custom, limit).await?,
            })
        }
        .await;
        match operation {
            Ok(results) => Ok(results),
            Err(SearchError::InvalidQuery(message)) => Err(SearchError::InvalidQuery(message)),
            Err(SearchError::Storage(source)) => {
                Err(SearchError::Storage(FoodStoreError::new(source)))
            }
        }
    }
}

async fn search_dataset(
    connection: &mut SqliteConnection,
    query: &SearchQuery,
    dataset: Dataset,
    limit: FoodLimit,
) -> Result<Vec<StoredFood>, SearchError<sqlx::Error>> {
    let rows = match sqlx::query_as::<_, StoredFoodRow>(SEARCH_SQL)
        .bind(query.as_str())
        .bind(dataset.as_str())
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
                .bind(dataset.as_str())
                .bind(1_i64)
                .fetch_optional(&mut *connection)
                .await
            {
                Ok(_) => return Err(SearchError::InvalidQuery(message)),
                Err(probe_error) => return Err(SearchError::Storage(probe_error)),
            }
        }
    };

    rows.into_iter()
        .map(|row| row.into_result().map_err(SearchError::Storage))
        .collect()
}

const SEARCH_SQL: &str = "
SELECT f.id, f.source, f.source_food_id, f.dataset,
       f.description, f.category, f.notes, f.parsed_data
FROM crumbs_foods_fts
JOIN crumbs_foods AS f ON f.id = crumbs_foods_fts.rowid
WHERE crumbs_foods_fts MATCH ? AND f.dataset = ?
ORDER BY crumbs_foods_fts.rank ASC, f.id ASC
LIMIT ?
";

const FTS_ERROR_PROBE_QUERY: &str = "crumbsftsvalidation";

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
#[path = "../../../unit-tests/foods/sqlite/search.rs"]
mod tests;
