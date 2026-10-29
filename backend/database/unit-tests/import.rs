use std::error::Error as StdError;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::{FromRow, query, query_as, query_scalar};

use super::ImportError;
use crate::{Database, DatabaseConfig};

const FOUNDATION_JSON: &str = include_str!("../test-fixtures/foundation.json");
const BRANDED_JSON: &str = include_str!("../test-fixtures/branded.json");
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
    assert_eq!(food.tags, "");
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
    assert_eq!(food.tags, "");
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
    query(
        "INSERT INTO crumbs_foods (
            source, source_food_id, food_type, description, category, tags, source_data
         ) VALUES ('other', '321358', 'Foundation', 'Other source food', NULL, 'other', '{}')",
    )
    .execute(&database.pool)
    .await?;

    database.import_usda(&path).await?;

    let other = food_row(&database, "other", "321358").await?;
    assert_eq!(other.description, "Other source food");
    assert_eq!(other.tags, "other");
    assert_eq!(other.source_data, "{}");
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
    tags: String,
    source_data: String,
}

async fn food_row(
    database: &Database,
    source: &str,
    source_food_id: &str,
) -> Result<FoodRow, sqlx::Error> {
    query_as(
        "SELECT id, source, food_type, description, category, tags, source_data
         FROM crumbs_foods
         WHERE source = ? AND source_food_id = ?",
    )
    .bind(source)
    .bind(source_food_id)
    .fetch_one(&database.pool)
    .await
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
