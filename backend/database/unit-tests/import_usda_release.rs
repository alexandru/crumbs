//! Official 2026-10-29 USDA Foundation Foods regression test (CC0 1.0).
//! https://fdc.nal.usda.gov/fdc-datasets/FoodData_Central_foundation_food_json_2026-10-29.zip

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sqlx::{SqlitePool, query_scalar};

use crate::{Database, DatabaseConfig};

const RELEASE_JSON_ENV: &str = "USDA_FOUNDATION_JSON";
const ENV_HELP: &str = "USDA_FOUNDATION_JSON must point at the official 2026-10-29 Foundation JSON";
const PINNED_RELEASE: &str = "2026-10-29";
const FOUNDATION_FOOD_COUNT: usize = 424;
const EXPECTED_SOURCE: &str = "usda";
const EXPECTED_FOOD_TYPE: &str = "Foundation";

type FoodId = i64;
type Source = String;
type SourceFoodId = String;
type FoodType = String;
type FoodDescription = String;
type FoodCategory = String;
type FoodTags = String;
type SourceData = String;
type ReferenceFoods = BTreeMap<SourceFoodId, Value>;
type TestResult = Result<(), Box<dyn StdError>>;

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct StoredFood {
    #[sqlx(rename = "id")]
    local_id: FoodId,
    source: Source,
    source_food_id: SourceFoodId,
    food_type: FoodType,
    description: FoodDescription,
    category: Option<FoodCategory>,
    tags: FoodTags,
    source_data: SourceData,
}

#[tokio::test]
#[ignore = "requires USDA_FOUNDATION_JSON pointing to the official October 2026 Foundation download"]
async fn imports_full_october_2026_foundation_release_idempotently() -> TestResult {
    let json_path = foundation_json_path()?;
    let reference = load_reference(&json_path)?;

    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("crumbs.db");
    let database = Database::open(config(database_path.clone())).await?;
    let applied_before = applied_migrations(&database.pool).await?;
    assert!(applied_before > 0, "no applied migration");

    let first_report = database.import_usda(json_path.as_path()).await?;
    assert_eq!(first_report.committed().get(), 424, "first import count");
    let first = read_stored(&database).await?;
    verify_reference(&first, &reference)?;
    verify_fts(&database.pool, &first).await?;
    verify_foreign_keys(&database.pool).await?;
    database.close().await;

    let reopened = Database::open(config(database_path)).await?;
    let applied_after_reopen = applied_migrations(&reopened.pool).await?;
    assert_eq!(applied_after_reopen, applied_before, "reopen migrations");

    let second_report = reopened.import_usda(json_path.as_path()).await?;
    assert_eq!(second_report.committed().get(), 424, "reimport count");
    let second = read_stored(&reopened).await?;
    assert_eq!(first, second, "reimport changed stored foods");
    verify_fts(&reopened.pool, &second).await?;
    verify_foreign_keys(&reopened.pool).await?;
    let applied_reimport = applied_migrations(&reopened.pool).await?;
    assert_eq!(applied_reimport, applied_before, "reimport migrations");
    reopened.close().await;

    Ok(())
}

fn foundation_json_path() -> Result<PathBuf, Box<dyn StdError>> {
    let path = std::env::var_os(RELEASE_JSON_ENV)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, ENV_HELP))?;
    Ok(PathBuf::from(path))
}

fn load_reference(path: &Path) -> Result<ReferenceFoods, Box<dyn StdError>> {
    let bytes = std::fs::read(path).map_err(|error| {
        io::Error::new(error.kind(), format!("read {}: {error}", path.display()))
    })?;
    let root: Value = serde_json::from_slice(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("parse {}: {error}", path.display()),
        )
    })?;
    let foods = root
        .as_object()
        .and_then(|root| root.get("FoundationFoods"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_data("root must contain a FoundationFoods array"))?;
    if foods.len() != FOUNDATION_FOOD_COUNT {
        return Err(invalid_data(format!(
            "expected {FOUNDATION_FOOD_COUNT} FoundationFoods for {PINNED_RELEASE}, found {}",
            foods.len()
        )));
    }

    let mut reference = BTreeMap::new();
    for food in foods {
        let source_food_id = food
            .get("fdcId")
            .and_then(Value::as_i64)
            .ok_or_else(|| invalid_data("missing fdcId"))?
            .to_string();
        if reference
            .insert(source_food_id.clone(), food.clone())
            .is_some()
        {
            return Err(invalid_data(format!("duplicate fdcId {source_food_id}")));
        }
    }
    Ok(reference)
}

async fn read_stored(database: &Database) -> Result<Vec<StoredFood>, sqlx::Error> {
    sqlx::query_as::<_, StoredFood>(
        "SELECT id, source, source_food_id, food_type, description, category, tags, source_data
         FROM crumbs_foods
         ORDER BY id",
    )
    .fetch_all(&database.pool)
    .await
}

fn verify_reference(stored: &[StoredFood], reference: &ReferenceFoods) -> TestResult {
    assert_eq!(stored.len(), FOUNDATION_FOOD_COUNT, "row count");
    for food in stored {
        let source_food_id = food.source_food_id.as_str();
        let Some(expected) = reference.get(source_food_id) else {
            return Err(invalid_data(format!("unknown {source_food_id}")));
        };
        let description = expected
            .get("description")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_data(format!("no description for {source_food_id}")))?;
        let category = match expected.get("foodCategory") {
            None | Some(Value::Null) => None,
            Some(category) => category
                .get("description")
                .and_then(Value::as_str)
                .map(Some)
                .ok_or_else(|| invalid_data(format!("bad category for {source_food_id}")))?,
        };
        assert_eq!(food.source, EXPECTED_SOURCE, "source {source_food_id}");
        assert_eq!(food.food_type, EXPECTED_FOOD_TYPE, "type {source_food_id}");
        assert_eq!(
            food.description, description,
            "description {source_food_id}"
        );
        assert_eq!(
            food.category.as_deref(),
            category,
            "category {source_food_id}"
        );
        assert!(food.tags.is_empty(), "tags {source_food_id}");
        let stored_value: Value = serde_json::from_str(&food.source_data).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid source_data {source_food_id}: {error}"),
            )
        })?;
        assert!(stored_value == *expected, "source_data {source_food_id}");
    }
    Ok(())
}

async fn verify_fts(pool: &SqlitePool, foods: &[StoredFood]) -> TestResult {
    for food in foods {
        let local_id = food.local_id;
        let description_query = format!("description : {}", fts_phrase(&food.description));
        let description_matches = matching_rowids(pool, &description_query).await?;
        assert!(
            description_matches.contains(&local_id),
            "FTS description {local_id}"
        );

        if let Some(category) = &food.category {
            let category_query = format!("category : {}", fts_phrase(category));
            let category_matches = matching_rowids(pool, &category_query).await?;
            assert!(
                category_matches.contains(&local_id),
                "FTS category {local_id}"
            );
        }
    }
    Ok(())
}

async fn matching_rowids(pool: &SqlitePool, fts_query: &str) -> Result<Vec<FoodId>, sqlx::Error> {
    query_scalar("SELECT rowid FROM crumbs_foods_fts WHERE crumbs_foods_fts MATCH ? ORDER BY rowid")
        .bind(fts_query)
        .fetch_all(pool)
        .await
}

fn fts_phrase(text: &str) -> String {
    // FTS5 escapes a double quote inside a quoted phrase by doubling it.
    format!("\"{}\"", text.replace('"', "\"\""))
}

async fn verify_foreign_keys(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await?;
    let violation_count = violations.len();
    assert!(
        violations.is_empty(),
        "foreign_key_check: {violation_count}"
    );
    Ok(())
}

async fn applied_migrations(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
        .fetch_one(pool)
        .await
}

fn config(path: PathBuf) -> DatabaseConfig {
    DatabaseConfig { path }
}

fn invalid_data(message: impl Into<String>) -> Box<dyn StdError> {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}
