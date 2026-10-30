use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use sqlx::{FromRow, query, query_as, query_scalar};

use super::ImportError;
use crate::{Database, DatabaseConfig, Food, FoodId, FoodStore, FoodWriter, TagName};

const FOUNDATION_JSON: &str = include_str!("../../test-fixtures/foundation.json");
const BRANDED_JSON: &str = include_str!("../../test-fixtures/branded.json");
const IMPORT_TIMEOUT: Duration = Duration::from_secs(5);
const LATER_TRIGGER_ROWS: usize = 100;

#[tokio::test]
async fn imports_foundation_foods_and_preserves_source_details() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), "foundation.json", FOUNDATION_JSON)?;

    let report = database.import_usda(&path).await?;

    assert_eq!(report.committed().get(), 2);
    assert_eq!(food_count(&database).await?, 2);
    let food = food_row(&database, "usda", "321358").await?;
    assert!(food.id > 0);
    assert_eq!(food.source, "usda");
    assert_eq!(food.food_type, "Foundation");
    assert_eq!(food.description, "Hummus, commercial");
    assert_eq!(
        food.category.as_deref(),
        Some("Legumes and Legume Products")
    );
    assert!(database.list_tags().await?.is_empty());
    assert!(food.source_data.contains(r#""unitName": "µg""#));
    assert!(food.source_data.contains(r#""amount": 3.0"#));
    assert!(food.source_data.contains(r#""name": "tablespoon""#));
    assert!(food.source_data.contains(r#""abbreviation": "tbsp""#));
    assert!(food.source_data.contains(r#""gramWeight": 33.9"#));
    assert!(
        food.source_data
            .contains(r#""publicationDate": "4/1/2019""#)
    );
    Ok(())
}

#[tokio::test]
async fn imports_branded_food_and_preserves_gtin_text() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), "branded.json", BRANDED_JSON)?;

    let report = database.import_usda(&path).await?;

    assert_eq!(report.committed().get(), 1);
    let food = food_row(&database, "usda", "1106281").await?;
    assert_eq!(food.source, "usda");
    assert_eq!(food.food_type, "Branded");
    assert_eq!(food.description, "GRANOLA, CINNAMON, RAISIN");
    assert_eq!(food.category.as_deref(), Some("Cereal"));
    assert!(database.list_tags().await?.is_empty());
    assert!(food.source_data.contains(r#""gtinUpc": "001633636543505""#));
    assert!(
        food.source_data
            .contains(r#""publicationDate": "11/13/2020""#)
    );
    assert!(food.source_data.contains(r#""labelNutrients""#));
    Ok(())
}

#[tokio::test]
async fn reimport_updates_in_place_and_retains_internal_id() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), "foundation.json", FOUNDATION_JSON)?;
    database.import_usda(&path).await?;
    let id_before = food_row(&database, "usda", "321358").await?.id;

    query(
        "UPDATE crumbs_foods
         SET description = 'stale', category = NULL, source_data = '{\"stale\":true}'
         WHERE source = 'usda' AND source_food_id = '321358'",
    )
    .execute(&database.pool)
    .await?;

    let report = database.import_usda(&path).await?;

    assert_eq!(report.committed().get(), 2);
    assert_eq!(food_count(&database).await?, 2);
    let food = food_row(&database, "usda", "321358").await?;
    assert_eq!(food.id, id_before, "upsert replaced the internal row id");
    assert_eq!(food.description, "Hummus, commercial");
    assert_eq!(
        food.category.as_deref(),
        Some("Legumes and Legume Products")
    );
    assert!(food.source_data.contains(r#""unitName": "µg""#));
    Ok(())
}

#[tokio::test]
async fn cross_source_same_food_id_is_isolated() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), "foundation.json", FOUNDATION_JSON)?;
    database.import_usda(&path).await?;
    query("INSERT INTO crumbs_sources (id) VALUES ('other')")
        .execute(&database.pool)
        .await?;
    let other_food_id: i64 = query_scalar(
        "INSERT INTO crumbs_foods (
            source, source_food_id, food_type, description, category, source_data
         ) VALUES ('other', '321358', 'Foundation', 'Other source food', NULL, '{}')
         RETURNING id",
    )
    .fetch_one(&database.pool)
    .await?;
    let other_tag = TagName::new("other")?;
    database
        .set_food_tags(FoodId(other_food_id), std::slice::from_ref(&other_tag))
        .await?;

    database.import_usda(&path).await?;

    let other = food_row(&database, "other", "321358").await?;
    assert_eq!(other.description, "Other source food");
    assert_eq!(other.source_data, "{}");
    let tagged_foods = database.foods_with_tag(&other_tag).await?;
    assert_eq!(tagged_foods.len(), 1);
    let tagged_food = tagged_foods
        .first()
        .ok_or_else(|| io::Error::other("tag lookup returned no foods"))?;
    assert_eq!(tagged_food.source, "other");
    assert_eq!(tagged_food.source_food_id, "321358");
    let usda = food_row(&database, "usda", "321358").await?;
    assert_eq!(usda.description, "Hummus, commercial");
    assert_eq!(food_count(&database).await?, 3);
    Ok(())
}

#[tokio::test]
async fn importing_a_subset_retains_absent_rows_and_coexists() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let foundation = write_input(directory.path(), "foundation.json", FOUNDATION_JSON)?;
    let branded = write_input(directory.path(), "branded.json", BRANDED_JSON)?;

    database.import_usda(&foundation).await?;
    let report = database.import_usda(&branded).await?;

    assert_eq!(report.committed().get(), 1);
    assert_eq!(food_count(&database).await?, 3);
    food_row(&database, "usda", "321358").await?;
    food_row(&database, "usda", "321360").await?;
    food_row(&database, "usda", "1106281").await?;
    Ok(())
}

#[tokio::test]
async fn imports_supported_roots_with_exact_food_types() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let sr = write_input(
        directory.path(),
        "sr.json",
        r#"{"SRLegacyFoods":[{"fdcId":9301,"description":"Legacy food","dataType":"SR Legacy"}]}"#,
    )?;
    let survey = write_input(
        directory.path(),
        "survey.json",
        r#"{"SurveyFoods":[{"fdcId":9302,"description":"Survey food","dataType":"Survey (FNDDS)"}]}"#,
    )?;

    database.import_usda(&sr).await?;
    database.import_usda(&survey).await?;

    let sr_food = food_row(&database, "usda", "9301").await?;
    let survey_food = food_row(&database, "usda", "9302").await?;
    assert_eq!(sr_food.food_type, "SR Legacy");
    assert_eq!(survey_food.food_type, "Survey (FNDDS)");
    Ok(())
}

#[tokio::test]
async fn empty_recognized_array_succeeds_with_zero_commits() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), "empty.json", r#"{"FoundationFoods":[]}"#)?;

    let report = database.import_usda(&path).await?;

    assert_eq!(report.committed().get(), 0);
    assert_eq!(food_count(&database).await?, 0);
    Ok(())
}

#[tokio::test]
async fn rejects_empty_malformed_unknown_and_mismatched_inputs() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let cases = [
        ("", "invalid USDA FoodData Central document"),
        ("{}", "empty object"),
        (r#"{"OtherFoods":[]}"#, "unsupported USDA root"),
        (r#"{"FoundationFoods":{}}"#, "invalid type"),
        (r#"{"FoundationFoods":[{"fdcId":1,}]}"#, "record 1"),
        (
            r#"{"FoundationFoods":[{"fdcId":1,"description":"Food","dataType":"Branded"}]}"#,
            "does not match",
        ),
    ];

    for (index, (contents, message)) in cases.into_iter().enumerate() {
        let name = format!("invalid-{index}.json");
        let path = write_input(directory.path(), &name, contents)?;
        let error = require_import_error(&database, &path).await?;

        assert!(
            error.to_string().contains(message),
            "unexpected error for {contents:?}: {error}"
        );
        assert!(error.to_string().contains("committed 0 foods"));
        assert!(error.to_string().contains(&path.display().to_string()));
        assert_eq!(food_count(&database).await?, 0);
    }
    Ok(())
}

#[tokio::test]
async fn late_framing_error_keeps_committed_food() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(
        directory.path(),
        "late.json",
        r#"{"FoundationFoods":[{"fdcId":9001,"description":"Late food","dataType":"Foundation"}]} trailing"#,
    )?;

    let error = require_import_error(&database, &path).await?;

    assert!(error.to_string().contains("committed 1 foods"), "{error}");
    assert!(error.source().is_some(), "import error lost its source");
    assert!(
        error.source().and_then(StdError::source).is_some(),
        "stream error lost its nested JSON source"
    );
    assert_eq!(food_count(&database).await?, 1);
    food_row(&database, "usda", "9001").await?;
    Ok(())
}

#[tokio::test]
async fn parse_failure_after_valid_food_keeps_earlier_commit_and_retry_succeeds()
-> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(
        directory.path(),
        "partial.json",
        r#"{"FoundationFoods":[
            {"fdcId":9101,"description":"First food","dataType":"Foundation"},
            {"fdcId":9102,"description":"  ","dataType":"Foundation"}
        ]}"#,
    )?;

    let error = require_import_error(&database, &path).await?;

    assert!(error.to_string().contains("record 2"), "{error}");
    assert!(error.to_string().contains("committed 1 foods"), "{error}");
    assert_eq!(food_count(&database).await?, 1);
    food_row(&database, "usda", "9101").await?;

    fs::write(
        &path,
        r#"{"FoundationFoods":[
            {"fdcId":9101,"description":"First food","dataType":"Foundation"},
            {"fdcId":9102,"description":"Second food","dataType":"Foundation"}
        ]}"#,
    )?;
    let report = database.import_usda(&path).await?;

    assert_eq!(report.committed().get(), 2);
    assert_eq!(food_count(&database).await?, 2);
    food_row(&database, "usda", "9102").await?;
    Ok(())
}

#[tokio::test]
async fn write_failure_rolls_back_only_failed_food_and_retry_succeeds()
-> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let input = trigger_failure_input();
    let path = write_input(directory.path(), "trigger.json", &input)?;
    query(
        "CREATE TRIGGER fail_trigger_food
         BEFORE INSERT ON crumbs_foods
         WHEN NEW.description = 'Trigger fail food'
         BEGIN
             SELECT RAISE(ABORT, 'induced write failure');
         END",
    )
    .execute(&database.pool)
    .await?;

    let error = require_import_error(&database, &path).await?;

    assert!(error.to_string().contains("9202"), "{error}");
    assert!(error.to_string().contains("record 2"), "{error}");
    assert!(error.to_string().contains("committed 1 foods"), "{error}");
    assert!(
        error.to_string().contains("induced write failure"),
        "{error}"
    );
    assert!(error.source().is_some(), "import error lost its source");
    assert_eq!(food_count(&database).await?, 1);
    food_row(&database, "usda", "9201").await?;
    assert_row_absent(&database, "usda", "9202").await?;
    assert_row_absent(&database, "usda", "9203").await?;
    assert_row_absent(&database, "usda", "9302").await?;

    query("DROP TRIGGER fail_trigger_food")
        .execute(&database.pool)
        .await?;
    let report = database.import_usda(&path).await?;

    let expected = 2 + LATER_TRIGGER_ROWS;
    assert_eq!(report.committed().get(), expected);
    assert_eq!(food_count(&database).await?, expected as i64);
    food_row(&database, "usda", "9202").await?;
    food_row(&database, "usda", "9203").await?;
    food_row(&database, "usda", "9302").await?;
    Ok(())
}

#[tokio::test]
async fn closed_pool_import_fails_with_bounded_termination() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), "closed.json", FOUNDATION_JSON)?;
    database.close().await;

    let error = require_import_error(&database, &path).await?;

    let message = error.to_string();
    assert!(
        message.contains("failed to persist USDA food 321358 at record 1"),
        "{message}"
    );
    assert!(message.contains("committed 0 foods"), "{message}");
    assert!(message.contains(&path.display().to_string()), "{message}");
    Ok(())
}

#[tokio::test]
async fn missing_input_reports_path_and_zero_committed() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = directory.path().join("missing.json");

    let error = require_import_error(&database, &path).await?;

    assert!(error.to_string().contains(&path.display().to_string()));
    assert!(error.to_string().contains("committed 0 foods"));
    assert_eq!(food_count(&database).await?, 0);
    Ok(())
}

#[tokio::test]
async fn imports_into_generic_food_writer_and_preserves_raw_json() -> Result<(), Box<dyn StdError>>
{
    let directory = tempfile::tempdir()?;
    let first_record = r#"{"fdcId":7301, "description":"Alternative food", "dataType":"Foundation", "foodCategory":{"description":"Shelf Stable"}, "details":{"measurement":1.00}}"#;
    let first_input = format!(r#"{{"FoundationFoods":[{first_record}]}}"#);
    let first_path = write_input(directory.path(), "first.json", &first_input)?;
    let store = RecordingFoodWriter::default();

    let report = crate::import_usda(&store, &first_path).await?;

    assert_eq!(report.committed().get(), 1);
    let first_food = store
        .snapshot()?
        .into_iter()
        .find(|food| food.source_food_id == "7301")
        .ok_or_else(|| io::Error::other("recording store did not receive the food"))?;
    assert_eq!(first_food.source, "usda");
    assert_eq!(first_food.food_type, "Foundation");
    assert_eq!(first_food.description, "Alternative food");
    assert_eq!(first_food.category.as_deref(), Some("Shelf Stable"));
    assert_eq!(first_food.source_data, first_record);

    let second_record = r#"{"fdcId":7301, "description":"Updated alternative food", "dataType":"Foundation", "foodCategory":{"description":"Shelf Stable"}, "details":{"measurement":2.00}}"#;
    let second_input = format!(r#"{{"FoundationFoods":[{second_record}]}}"#);
    let second_path = write_input(directory.path(), "second.json", &second_input)?;
    let report = crate::import_usda(&store, &second_path).await?;

    assert_eq!(report.committed().get(), 1);
    let updated_food = store
        .snapshot()?
        .into_iter()
        .find(|food| food.source_food_id == "7301")
        .ok_or_else(|| io::Error::other("recording store lost the food"))?;
    assert_eq!(updated_food.id, first_food.id);
    assert_eq!(updated_food.description, "Updated alternative food");
    assert_eq!(updated_food.source_data, second_record);
    assert_eq!(store.snapshot()?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn generic_writer_failure_reports_commits_and_joins_producer() -> Result<(), Box<dyn StdError>>
{
    let directory = tempfile::tempdir()?;
    let input = trigger_failure_input();
    let path = write_input(directory.path(), "write-failure.json", &input)?;
    let store = RecordingFoodWriter {
        fail_after: Some(1),
        ..RecordingFoodWriter::default()
    };

    let error = require_writer_import_error(&store, &path).await?;

    assert!(error.to_string().contains("9202"), "{error}");
    assert!(error.to_string().contains("record 2"), "{error}");
    assert!(error.to_string().contains("committed 1 foods"), "{error}");
    assert!(
        error.to_string().contains("injected storage failure"),
        "{error}"
    );
    let foods = store.snapshot()?;
    assert_eq!(foods.len(), 1);
    let committed_food = foods
        .first()
        .ok_or_else(|| io::Error::other("recording store did not retain the first food"))?;
    assert_eq!(committed_food.source_food_id, "9201");
    Ok(())
}

async fn open_database() -> Result<(tempfile::TempDir, Database), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let database = Database::open(DatabaseConfig {
        path: directory.path().join("crumbs.db"),
    })
    .await?;
    Ok((directory, database))
}

fn write_input(directory: &Path, name: &str, contents: &str) -> Result<PathBuf, Box<dyn StdError>> {
    let path = directory.join(name);
    fs::write(&path, contents)?;
    Ok(path)
}

fn trigger_failure_input() -> String {
    let mut input = String::from(
        r#"{"FoundationFoods":[
            {"fdcId":9201,"description":"Good food","dataType":"Foundation"},
            {"fdcId":9202,"description":"Trigger fail food","dataType":"Foundation"}"#,
    );
    for offset in 1..=LATER_TRIGGER_ROWS {
        let fdc_id = 9202 + offset;
        let _ = write!(
            &mut input,
            r#",{{"fdcId":{fdc_id},"description":"After failure","dataType":"Foundation"}}"#
        );
    }
    input.push_str("]}");
    input
}

async fn require_import_error(
    database: &Database,
    path: &Path,
) -> Result<ImportError, Box<dyn StdError>> {
    let result = tokio::time::timeout(IMPORT_TIMEOUT, database.import_usda(path))
        .await
        .map_err(|_| {
            io::Error::other(format!(
                "import did not terminate within {} seconds",
                IMPORT_TIMEOUT.as_secs()
            ))
        })?;

    match result {
        Ok(report) => Err(io::Error::other(format!(
            "import unexpectedly committed {} foods",
            report.committed().get()
        ))
        .into()),
        Err(error) => Ok(error),
    }
}

async fn require_writer_import_error<S: FoodWriter + ?Sized>(
    store: &S,
    path: &Path,
) -> Result<ImportError, Box<dyn StdError>> {
    let result = tokio::time::timeout(IMPORT_TIMEOUT, crate::import_usda(store, path))
        .await
        .map_err(|_| {
            io::Error::other(format!(
                "import did not terminate within {} seconds",
                IMPORT_TIMEOUT.as_secs()
            ))
        })?;

    match result {
        Ok(report) => Err(io::Error::other(format!(
            "import unexpectedly committed {} foods",
            report.committed().get()
        ))
        .into()),
        Err(error) => Ok(error),
    }
}

async fn food_count(database: &Database) -> Result<i64, sqlx::Error> {
    query_scalar("SELECT COUNT(*) FROM crumbs_foods")
        .fetch_one(&database.pool)
        .await
}

#[derive(Debug, FromRow)]
struct FoodRow {
    id: i64,
    source: String,
    food_type: String,
    description: String,
    category: Option<String>,
    source_data: String,
}

async fn food_row(
    database: &Database,
    source: &str,
    source_food_id: &str,
) -> Result<FoodRow, sqlx::Error> {
    query_as(
        "SELECT id, source, food_type, description, category, source_data
         FROM crumbs_foods
         WHERE source = ? AND source_food_id = ?",
    )
    .bind(source)
    .bind(source_food_id)
    .fetch_one(&database.pool)
    .await
}

#[derive(Default)]
struct RecordingFoodWriter {
    state: Mutex<RecordingFoodState>,
    fail_after: Option<usize>,
}

#[derive(Default)]
struct RecordingFoodState {
    foods: HashMap<(String, String), RecordedFood>,
    last_id: i64,
    successful_writes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedFood {
    id: FoodId,
    source: String,
    source_food_id: String,
    food_type: String,
    description: String,
    category: Option<String>,
    source_data: String,
}

impl RecordingFoodWriter {
    fn snapshot(&self) -> Result<Vec<RecordedFood>, io::Error> {
        let state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("recording food writer mutex was poisoned"))?;
        let mut foods: Vec<_> = state.foods.values().cloned().collect();
        foods.sort_by_key(|food| food.id.0);
        Ok(foods)
    }
}

impl FoodWriter for RecordingFoodWriter {
    type Error = io::Error;

    async fn upsert_food(&self, food: &Food) -> Result<FoodId, Self::Error> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("recording food writer mutex was poisoned"))?;

        if let Some(limit) = self.fail_after
            && state.successful_writes >= limit
        {
            return Err(io::Error::other("injected storage failure"));
        }

        let source = food.source.to_string();
        let source_food_id = food.source_food_id.clone();
        let key = (source.clone(), source_food_id.clone());
        let id = match state.foods.get(&key) {
            Some(previous) => previous.id,
            None => {
                let next_id = state
                    .last_id
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("recording food ID overflow"))?;
                state.last_id = next_id;
                FoodId(next_id)
            }
        };
        let successful_writes = state
            .successful_writes
            .checked_add(1)
            .ok_or_else(|| io::Error::other("recording write count overflow"))?;
        let recorded_food = RecordedFood {
            id,
            source,
            source_food_id,
            food_type: food.food_type.to_string(),
            description: food.description.clone(),
            category: food.category.clone(),
            source_data: food.source_data.get().to_owned(),
        };

        state.foods.insert(key, recorded_food);
        state.successful_writes = successful_writes;
        Ok(id)
    }
}

async fn assert_row_absent(
    database: &Database,
    source: &str,
    source_food_id: &str,
) -> Result<(), Box<dyn StdError>> {
    let count: i64 = query_scalar(
        "SELECT COUNT(*) FROM crumbs_foods
         WHERE source = ? AND source_food_id = ?",
    )
    .bind(source)
    .bind(source_food_id)
    .fetch_one(&database.pool)
    .await?;
    assert_eq!(count, 0, "unexpected food row {source}/{source_food_id}");
    Ok(())
}
