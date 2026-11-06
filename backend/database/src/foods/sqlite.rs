use std::collections::HashSet;

use sqlx::error::DatabaseError;
use sqlx::sqlite::SqliteError;
use sqlx::{FromRow, Row, SqliteConnection};

use super::{
    CustomFoodInput, CustomFoodStore, Dataset, Food, FoodCategory, FoodDescription, FoodId,
    FoodNotes, FoodReader, FoodSearchResult, FoodSearchResults, FoodSummary, FoodWriter,
    SearchError, SearchLimit, SearchQuery, Source, SourceFoodId, Tag, TagId, TagKey, TagName,
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
                source, source_food_id, dataset, description, category, notes, parsed_data, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (source, source_food_id) DO UPDATE SET
                dataset = excluded.dataset,
                description = excluded.description,
                category = excluded.category,
                notes = excluded.notes,
                parsed_data = excluded.parsed_data,
                updated_at = ?
            RETURNING id",
        )
        .bind(food.source.as_ref())
        .bind(&food.source_food_id)
        .bind(food.dataset.as_str())
        .bind(&food.description)
        .bind(food.category.as_deref())
        .bind(food.notes.as_deref())
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
    ) -> Result<FoodSearchResults, SearchError<Self::Error>> {
        let mut connection = self.pool.acquire().await.map_err(SearchError::Storage)?;
        Ok(FoodSearchResults {
            common: search_dataset(&mut connection, query, Dataset::Common, limit).await?,
            branded: search_dataset(&mut connection, query, Dataset::Branded, limit).await?,
            custom: search_dataset(&mut connection, query, Dataset::Custom, limit).await?,
        })
    }
}

impl CustomFoodStore for Database {
    type Error = sqlx::Error;

    async fn create_custom_food(
        &self,
        input: &CustomFoodInput,
    ) -> Result<FoodSearchResult, Self::Error> {
        let now: UnixMillis = self.clock.now()?;
        let parsed_data = serde_json::to_string(&input.parsed_data)
            .map_err(|source| sqlx::Error::Encode(Box::new(source)))?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO crumbs_sources (id, created_at) VALUES ('custom', ?)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let row = sqlx::query_as::<_, FoodSearchRow>(
            "INSERT INTO crumbs_foods (
                source, source_food_id, dataset, description, category, notes, parsed_data, created_at
            ) VALUES ('custom', lower(hex(randomblob(16))), 'custom', ?, ?, ?, ?, ?)
            RETURNING id, source, source_food_id, dataset, description, category, notes, parsed_data",
        )
        .bind(input.description.as_str())
        .bind(input.category.as_deref())
        .bind(input.notes.as_deref())
        .bind(parsed_data)
        .bind(now)
        .fetch_one(&mut *transaction)
        .await?;
        let food = row.into_result()?;
        transaction.commit().await?;
        Ok(food)
    }

    async fn custom_food(&self, id: FoodId) -> Result<Option<FoodSearchResult>, Self::Error> {
        let row = sqlx::query_as::<_, FoodSearchRow>(
            "SELECT id, source, source_food_id, dataset, description, category, notes, parsed_data
             FROM crumbs_foods
             WHERE id = ? AND source = 'custom' AND dataset = 'custom'",
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await?;
        row.map(FoodSearchRow::into_result).transpose()
    }

    async fn list_custom_foods(
        &self,
        limit: SearchLimit,
    ) -> Result<Vec<FoodSearchResult>, Self::Error> {
        sqlx::query_as::<_, FoodSearchRow>(
            "SELECT id, source, source_food_id, dataset, description, category, notes, parsed_data
             FROM crumbs_foods
             WHERE source = 'custom' AND dataset = 'custom'
             ORDER BY id ASC
             LIMIT ?",
        )
        .bind(limit.get())
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(FoodSearchRow::into_result)
        .collect()
    }

    async fn update_custom_food(
        &self,
        id: FoodId,
        input: &CustomFoodInput,
    ) -> Result<Option<FoodSearchResult>, Self::Error> {
        let now: UnixMillis = self.clock.now()?;
        let parsed_data = serde_json::to_string(&input.parsed_data)
            .map_err(|source| sqlx::Error::Encode(Box::new(source)))?;
        let row = sqlx::query_as::<_, FoodSearchRow>(
            "UPDATE crumbs_foods
             SET description = ?, category = ?, notes = ?, parsed_data = ?, updated_at = ?
             WHERE id = ? AND source = 'custom' AND dataset = 'custom'
             RETURNING id, source, source_food_id, dataset, description, category, notes, parsed_data",
        )
        .bind(input.description.as_str())
        .bind(input.category.as_deref())
        .bind(input.notes.as_deref())
        .bind(parsed_data)
        .bind(now)
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await?;
        row.map(FoodSearchRow::into_result).transpose()
    }

    async fn delete_custom_food(&self, id: FoodId) -> Result<bool, Self::Error> {
        let deleted = sqlx::query_scalar::<_, RowId>(
            "DELETE FROM crumbs_foods
             WHERE id = ? AND source = 'custom' AND dataset = 'custom'
             RETURNING id",
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await?;
        Ok(deleted.is_some())
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
            "SELECT id, source, source_food_id, dataset, description, category, notes
             FROM crumbs_foods WHERE source = ? AND source_food_id = ?",
        )
        .bind(source.as_ref())
        .bind(source_food_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(FoodSummary::try_from).transpose()
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

async fn search_dataset(
    connection: &mut SqliteConnection,
    query: &SearchQuery,
    dataset: Dataset,
    limit: SearchLimit,
) -> Result<Vec<FoodSearchResult>, SearchError<sqlx::Error>> {
    let rows = match sqlx::query_as::<_, FoodSearchRow>(SEARCH_SQL)
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
    notes: Option<FoodNotes>,
    parsed_data: ParsedFoodDataText,
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

impl FoodSearchRow {
    fn into_result(self) -> Result<FoodSearchResult, sqlx::Error> {
        let dataset = self
            .dataset
            .parse::<Dataset>()
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        let parsed_data = serde_json::from_str::<FoodData>(&self.parsed_data)
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        Ok(FoodSearchResult {
            id: FoodId(self.id),
            food: Food {
                source: Source::Owned(self.source),
                source_food_id: self.source_food_id,
                dataset,
                description: self.description,
                category: self.category,
                notes: self.notes,
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
    notes: Option<FoodNotes>,
}

impl TryFrom<FoodRow> for FoodSummary {
    type Error = sqlx::Error;

    fn try_from(row: FoodRow) -> Result<Self, Self::Error> {
        let dataset = row
            .dataset
            .parse::<Dataset>()
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        Ok(Self {
            id: FoodId(row.id),
            source: row.source.into(),
            source_food_id: row.source_food_id,
            dataset,
            description: row.description,
            category: row.category,
            notes: row.notes,
        })
    }
}

#[cfg(test)]
#[path = "../../unit-tests/foods/sqlite.rs"]
mod tests;
