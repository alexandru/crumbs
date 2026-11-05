use std::collections::HashSet;

use sqlx::error::DatabaseError;
use sqlx::sqlite::SqliteError;
use sqlx::{FromRow, Row};

use super::{
    Dataset, Food, FoodCategory, FoodDescription, FoodId, FoodReader, FoodSearchResult,
    FoodSummary, FoodWriter, SearchError, SearchLimit, SearchQuery, Source, SourceFoodId, Tag,
    TagId, TagKey, TagName,
};
use crate::Database;
use crate::clock::{ClockError, UnixMillis};
use crate::nutrition::FoodData;

impl FoodWriter for Database {
    type Error = sqlx::Error;

    async fn upsert_food(&self, food: &Food) -> Result<FoodId, Self::Error> {
        let now: UnixMillis = self.clock.now()?;
        let parsed_data = serde_json::to_string(&food.parsed_data)
            .map_err(|source| sqlx::Error::Encode(Box::new(source)))?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO crumbs_sources (id, created_at) VALUES (?, ?)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(food.source.as_ref())
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let id = sqlx::query_scalar(
            "INSERT INTO crumbs_foods (
                source, source_food_id, dataset, description, category, parsed_data, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (source, source_food_id) DO UPDATE SET
                dataset = excluded.dataset,
                description = excluded.description,
                category = excluded.category,
                parsed_data = excluded.parsed_data,
                updated_at = ?
            RETURNING id",
        )
        .bind(food.source.as_ref())
        .bind(&food.source_food_id)
        .bind(food.dataset.as_ref())
        .bind(&food.description)
        .bind(food.category.as_deref())
        .bind(parsed_data)
        .bind(now)
        .bind(now)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(FoodId(id))
    }
}

impl FoodReader for Database {
    type Error = sqlx::Error;

    async fn search_foods(
        &self,
        query: &SearchQuery,
        limit: SearchLimit,
    ) -> Result<Vec<FoodSearchResult>, SearchError<Self::Error>> {
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

impl Database {
    /// Looks up food metadata by its exact source-scoped identity.
    /// Returns `None` if the identity is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if storage access fails.
    pub async fn food_by_source(
        &self,
        source: &Source,
        source_food_id: &SourceFoodId,
    ) -> Result<Option<FoodSummary>, sqlx::Error> {
        let row = sqlx::query_as::<_, FoodRow>(
            "SELECT id, source, source_food_id, dataset, description, category
             FROM crumbs_foods WHERE source = ? AND source_food_id = ?",
        )
        .bind(source.as_ref())
        .bind(source_food_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(FoodSummary::from))
    }

    /// Atomically replaces a food's assignments, creating missing tags.
    ///
    /// # Errors
    ///
    /// Returns an error if the food is missing or the replacement fails. Failed
    /// replacement leaves existing assignments unchanged and does not commit
    /// catalog changes.
    pub async fn set_food_tags(&self, food: FoodId, tags: &[TagName]) -> Result<(), sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM crumbs_foods WHERE id = ?)")
                .bind(food.0)
                .fetch_one(&mut *transaction)
                .await?;
        if !exists {
            return Err(sqlx::Error::RowNotFound);
        }
        let now: UnixMillis = self.clock.now()?;
        let assignments = sqlx::query_as::<_, TagAssignmentRow>(
            "SELECT t.id AS tag_id, t.name_key
             FROM crumbs_food_tags AS ft
             JOIN crumbs_tags AS t ON t.id = ft.tag_id
             WHERE ft.food_id = ?",
        )
        .bind(food.0)
        .fetch_all(&mut *transaction)
        .await?;
        let requested: HashSet<&str> = tags.iter().map(|tag| tag.key.as_str()).collect();
        for assignment in assignments {
            if !requested.contains(assignment.name_key.as_str()) {
                sqlx::query("DELETE FROM crumbs_food_tags WHERE food_id = ? AND tag_id = ?")
                    .bind(food.0)
                    .bind(assignment.tag_id)
                    .execute(&mut *transaction)
                    .await?;
            }
        }
        for tag in tags {
            sqlx::query(
                "INSERT INTO crumbs_tags (name, name_key, created_at) VALUES (?, ?, ?)
                 ON CONFLICT (name_key) DO NOTHING",
            )
            .bind(tag.as_str())
            .bind(&tag.key)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "INSERT INTO crumbs_food_tags (food_id, tag_id, created_at)
                 SELECT ?, id, ? FROM crumbs_tags WHERE name_key = ?
                 ON CONFLICT (food_id, tag_id) DO NOTHING",
            )
            .bind(food.0)
            .bind(now)
            .bind(&tag.key)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await
    }

    /// Lists the tag catalog, including tags with no current assignments.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage read fails.
    pub async fn list_tags(&self) -> Result<Vec<Tag>, sqlx::Error> {
        sqlx::query("SELECT id, name FROM crumbs_tags ORDER BY name_key")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|row| {
                Ok(Tag {
                    id: TagId(row.try_get("id")?),
                    name: TagName::new(row.try_get::<String, _>("name")?)
                        .map_err(|source| sqlx::Error::Decode(Box::new(source)))?,
                })
            })
            .collect()
    }
}

impl From<ClockError> for sqlx::Error {
    fn from(error: ClockError) -> Self {
        Self::Encode(Box::new(error))
    }
}

type RowId = i64;
type SourceText = String;
type DatasetText = String;
type ParsedFoodDataText = String;

#[derive(FromRow)]
struct FoodSearchRow {
    id: RowId,
    source: SourceText,
    source_food_id: SourceFoodId,
    dataset: DatasetText,
    description: FoodDescription,
    category: Option<FoodCategory>,
    parsed_data: ParsedFoodDataText,
}

const SEARCH_SQL: &str = "
SELECT f.id, f.source, f.source_food_id, f.dataset,
       f.description, f.category, f.parsed_data
FROM crumbs_foods_fts
JOIN crumbs_foods AS f ON f.id = crumbs_foods_fts.rowid
WHERE crumbs_foods_fts MATCH ?
ORDER BY crumbs_foods_fts.rank ASC, f.id ASC
LIMIT ?
";

const FTS_ERROR_PROBE_QUERY: &str = "crumbsftsvalidation";

impl FoodSearchRow {
    fn into_result(self) -> Result<FoodSearchResult, SearchError<sqlx::Error>> {
        let parsed_data = serde_json::from_str::<FoodData>(&self.parsed_data)
            .map_err(|error| SearchError::Storage(sqlx::Error::Decode(Box::new(error))))?;
        Ok(FoodSearchResult {
            id: FoodId(self.id),
            food: Food {
                source: Source::Owned(self.source),
                source_food_id: self.source_food_id,
                dataset: Dataset::Owned(self.dataset),
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

#[derive(FromRow)]
struct TagAssignmentRow {
    tag_id: RowId,
    name_key: TagKey,
}

#[derive(FromRow)]
struct FoodRow {
    id: RowId,
    source: SourceText,
    source_food_id: SourceFoodId,
    dataset: DatasetText,
    description: FoodDescription,
    category: Option<FoodCategory>,
}

impl From<FoodRow> for FoodSummary {
    fn from(row: FoodRow) -> Self {
        Self {
            id: FoodId(row.id),
            source: row.source.into(),
            source_food_id: row.source_food_id,
            dataset: row.dataset.into(),
            description: row.description,
            category: row.category,
        }
    }
}

#[cfg(test)]
#[path = "../../unit-tests/foods/sqlite.rs"]
mod tests;
