//! Official 2026-10-29 USDA Foundation Foods regression test (CC0 1.0).
//! https://fdc.nal.usda.gov/fdc-datasets/FoodData_Central_foundation_food_json_2026-10-29.zip

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::value::RawValue;
use sqlx::{SqlitePool, query, query_scalar};

use common::DatabaseFixture;
use crumbs_database::nutrition::{FoodData, Mass};
use crumbs_database::{Database, FoodId as DatabaseFoodId, TagName};
use crumbs_import::{DiagnosticKind, ImportDiagnostic, import_usda_with_diagnostics};

mod common;

const RELEASE_JSON_ENV: &str = "USDA_FOUNDATION_JSON";
const ENV_HELP: &str = "USDA_FOUNDATION_JSON must point at the official 2026-10-29 Foundation JSON";
const PINNED_RELEASE: ReleaseDate = "2026-10-29";
const FOUNDATION_FOOD_COUNT: FoodCount = 424;
const COMMITTED_FOOD_COUNT: FoodCount = 424;
const SANITIZED_FOOD_COUNT: FoodCount = 10;
const SANITIZED_FOOD_IDS: [&str; 10] = [
    "2727566", "2727567", "2727568", "2727569", "2727570", "2727571", "2727575", "2727576",
    "2747656", "2747673",
];
const EXPECTED_SOURCE: SourceRef<'_> = "usda";
const EXPECTED_DATASET: DatasetRef<'_> = "usda_foundation";

type RawFoodId = i64;
type ReleaseDate = &'static str;
type FoodCount = usize;
type SourceRef<'a> = &'a str;
type DatasetRef<'a> = &'a str;
type FtsQuery<'a> = &'a str;
type FtsPhrase = String;
type MigrationCount = i64;
type Source = String;
type SourceFoodId = String;
type RawFdcId = i64;
type RawNutrientId = i64;
type Dataset = String;
type FoodDescription = String;
type FoodCategory = String;
type FoodTags = String;
type ParsedData = String;
type TestResult = Result<(), Box<dyn StdError>>;

struct ReferenceFoods {
    by_id: BTreeMap<SourceFoodId, ReferenceFood>,
    order: Vec<SourceFoodId>,
}

#[derive(Debug, Deserialize)]
struct FoundationFoodsArchive {
    #[serde(rename = "FoundationFoods")]
    foods: Vec<ReferenceFood>,
}

#[derive(Debug, Deserialize)]
struct ReferenceFood {
    #[serde(rename = "fdcId")]
    fdc_id: RawFdcId,
    description: FoodDescription,
    #[serde(default, rename = "foodCategory")]
    food_category: Option<ReferenceFoodCategory>,
    #[serde(default, rename = "foodNutrients")]
    food_nutrients: Vec<ReferenceFoodNutrient>,
}

#[derive(Debug, Deserialize)]
struct ReferenceFoodCategory {
    description: Option<FoodCategory>,
}

#[derive(Debug, Deserialize)]
struct ReferenceFoodNutrient {
    #[serde(default)]
    amount: Option<Box<RawValue>>,
    #[serde(default)]
    nutrient: Option<ReferenceNutrient>,
}

#[derive(Debug, Deserialize)]
struct ReferenceNutrient {
    #[serde(default)]
    id: Option<RawNutrientId>,
    #[serde(default, rename = "unitName")]
    unit_name: Option<String>,
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct StoredFood {
    #[sqlx(rename = "id")]
    local_id: RawFoodId,
    source: Source,
    source_food_id: SourceFoodId,
    dataset: Dataset,
    description: FoodDescription,
    category: Option<FoodCategory>,
    tags: FoodTags,
    parsed_data: ParsedData,
}

#[tokio::test]
#[ignore = "requires USDA_FOUNDATION_JSON pointing to the official October 2026 Foundation download"]
async fn sanitizes_negative_fooddata_amount_and_preserves_foods_and_tags_on_retry() -> TestResult {
    let json_path = foundation_json_path()?;
    let reference = load_reference(&json_path)?;

    let mut fixture = DatabaseFixture::open().await?;
    let applied_before = applied_migrations(&fixture.pool).await?;
    assert!(applied_before > 0, "no applied migration");

    let (first_report, first_diagnostics) = import_release(&fixture.database, &json_path).await?;
    verify_report(first_report, &first_diagnostics);
    let mut first = read_stored(&fixture.pool).await?;
    verify_reference(&first, &reference)?;
    assert!(
        first.iter().all(|food| food.tags.is_empty()),
        "initial import assigned tags"
    );
    verify_negative_carbohydrates_absent(&first)?;
    let reviewed_tag = TagName::new("Reviewed")?;
    let (reviewed_food_id, reviewed_food_local_id) = match first.first_mut() {
        Some(food) => {
            food.tags = reviewed_tag.as_str().to_owned();
            (DatabaseFoodId(food.local_id), food.local_id)
        }
        None => return Err(invalid_data("first import returned no foods")),
    };
    fixture
        .database
        .set_food_tags(reviewed_food_id, std::slice::from_ref(&reviewed_tag))
        .await?;
    let tag_matches = matching_rowids(&fixture.pool, "tags:reviewed").await?;
    assert_eq!(tag_matches, vec![reviewed_food_local_id], "FTS tag");
    verify_fts(&fixture.pool, &first).await?;
    verify_foreign_keys(&fixture.pool).await?;
    fixture.reopen().await?;
    let applied_after_reopen = applied_migrations(&fixture.pool).await?;
    assert_eq!(applied_after_reopen, applied_before, "reopen migrations");

    let (second_report, second_diagnostics) = import_release(&fixture.database, &json_path).await?;
    verify_report(second_report, &second_diagnostics);
    let second = read_stored(&fixture.pool).await?;
    assert_eq!(first, second, "reimport changed stored foods");
    verify_reference(&second, &reference)?;
    verify_negative_carbohydrates_absent(&second)?;
    verify_fts(&fixture.pool, &second).await?;
    let tag_matches = matching_rowids(&fixture.pool, "tags:reviewed").await?;
    assert_eq!(
        tag_matches,
        vec![reviewed_food_local_id],
        "reopened and reimported FTS tag"
    );
    verify_foreign_keys(&fixture.pool).await?;
    let applied_reimport = applied_migrations(&fixture.pool).await?;
    assert_eq!(applied_reimport, applied_before, "reimport migrations");
    fixture.close().await;

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
    let archive: FoundationFoodsArchive = serde_json::from_slice(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("parse {}: {error}", path.display()),
        )
    })?;
    if archive.foods.len() != FOUNDATION_FOOD_COUNT {
        return Err(invalid_data(format!(
            "expected {FOUNDATION_FOOD_COUNT} FoundationFoods for {PINNED_RELEASE}, found {}",
            archive.foods.len()
        )));
    }

    let mut reference = ReferenceFoods {
        by_id: BTreeMap::new(),
        order: Vec::with_capacity(archive.foods.len()),
    };
    for food in archive.foods {
        let source_food_id = food.fdc_id.to_string();
        if reference
            .by_id
            .insert(source_food_id.clone(), food)
            .is_some()
        {
            return Err(invalid_data(format!("duplicate fdcId {source_food_id}")));
        }
        reference.order.push(source_food_id);
    }
    Ok(reference)
}

async fn read_stored(pool: &SqlitePool) -> Result<Vec<StoredFood>, sqlx::Error> {
    sqlx::query_as::<_, StoredFood>(
        "SELECT f.id, f.source, f.source_food_id, f.dataset, f.description, f.category,
                s.tags AS tags, f.parsed_data
         FROM crumbs_foods AS f
         JOIN crumbs_foods_search_content AS s ON s.id = f.id
         ORDER BY f.id",
    )
    .fetch_all(pool)
    .await
}

fn verify_reference(stored: &[StoredFood], reference: &ReferenceFoods) -> TestResult {
    assert_eq!(stored.len(), COMMITTED_FOOD_COUNT, "row count");
    for (food, expected_source_food_id) in stored.iter().zip(reference.order.iter()) {
        let source_food_id = food.source_food_id.as_str();
        assert_eq!(
            &food.source_food_id, expected_source_food_id,
            "committed source-food prefix"
        );
        let Some(expected) = reference.by_id.get(source_food_id) else {
            return Err(invalid_data(format!("unknown {source_food_id}")));
        };
        let description = &expected.description;
        let category = expected
            .food_category
            .as_ref()
            .and_then(|category| category.description.as_deref());
        assert_eq!(food.source, EXPECTED_SOURCE, "source {source_food_id}");
        assert_eq!(food.dataset, EXPECTED_DATASET, "dataset {source_food_id}");
        assert_eq!(
            food.description, *description,
            "description {source_food_id}"
        );
        assert_eq!(
            food.category.as_deref(),
            category,
            "category {source_food_id}"
        );
        let parsed_data: FoodData = serde_json::from_str(&food.parsed_data).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid parsed_data {source_food_id}: {error}"),
            )
        })?;
        verify_protein(&parsed_data, expected, source_food_id)?;
    }
    Ok(())
}

async fn import_release(
    database: &Database,
    path: &Path,
) -> Result<(crumbs_import::ImportReport, Vec<ImportDiagnostic>), Box<dyn StdError>> {
    let mut diagnostics = Vec::new();
    let report = import_usda_with_diagnostics(database, path, |diagnostic| {
        diagnostics.push(diagnostic.clone());
        Ok(())
    })
    .await?;
    Ok((report, diagnostics))
}

fn verify_report(report: crumbs_import::ImportReport, diagnostics: &[ImportDiagnostic]) {
    assert_eq!(
        report.committed().get(),
        COMMITTED_FOOD_COUNT,
        "committed count"
    );
    assert_eq!(
        report.sanitized().get(),
        SANITIZED_FOOD_COUNT,
        "sanitized count"
    );
    assert_eq!(report.rejected().get(), 0, "rejected count");
    assert_eq!(diagnostics.len(), SANITIZED_FOOD_COUNT, "diagnostic count");
    let actual_ids: Vec<&str> = diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.source, EXPECTED_SOURCE);
            assert_eq!(diagnostic.kind, DiagnosticKind::Sanitized);
            assert_eq!(
                diagnostic.field.as_deref(),
                Some("carbohydrates.including_fibre")
            );
            assert!(diagnostic.reason.contains("negative source amount"));
            diagnostic.source_food_id.as_deref().unwrap_or("unknown")
        })
        .collect();
    assert_eq!(actual_ids, SANITIZED_FOOD_IDS, "sanitized food IDs");
    for (diagnostic, expected_position) in diagnostics
        .iter()
        .zip([315, 316, 317, 318, 319, 320, 324, 325, 343, 360])
    {
        assert_eq!(diagnostic.position.get(), expected_position);
    }
}

fn verify_negative_carbohydrates_absent(stored: &[StoredFood]) -> TestResult {
    for source_food_id in SANITIZED_FOOD_IDS {
        let food = stored
            .iter()
            .find(|food| food.source_food_id == source_food_id)
            .ok_or_else(|| invalid_data(format!("missing sanitized food {source_food_id}")))?;
        let parsed_data: FoodData = serde_json::from_str(&food.parsed_data).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid parsed_data {source_food_id}: {error}"),
            )
        })?;
        let carbohydrate = parsed_data
            .nutrition
            .as_ref()
            .and_then(|profile| profile.carbohydrates.including_fibre.as_ref());
        assert_eq!(carbohydrate, None, "negative carbohydrate {source_food_id}");
    }
    Ok(())
}

fn verify_protein(
    parsed_data: &FoodData,
    expected: &ReferenceFood,
    source_food_id: &str,
) -> TestResult {
    let expected_food_nutrient = expected.food_nutrients.iter().find(|food_nutrient| {
        food_nutrient
            .nutrient
            .as_ref()
            .and_then(|nutrient| nutrient.id)
            == Some(1003)
    });
    let expected_protein = match expected_food_nutrient {
        Some(food_nutrient) => match food_nutrient.amount.as_ref() {
            Some(amount) => {
                let nutrient = food_nutrient.nutrient.as_ref().ok_or_else(|| {
                    invalid_data(format!("protein nutrient has no ID {source_food_id}"))
                })?;
                let unit = nutrient.unit_name.as_deref().ok_or_else(|| {
                    invalid_data(format!("protein nutrient has no unit {source_food_id}"))
                })?;
                let amount =
                    format!("{} {unit}", amount.get())
                        .parse::<Mass>()
                        .map_err(|source| {
                            invalid_data(format!(
                                "invalid reference protein {source_food_id}: {source}"
                            ))
                        })?;
                Some(amount.to_string())
            }
            None => None,
        },
        None => None,
    };
    let actual_protein = parsed_data
        .nutrition
        .as_ref()
        .and_then(|profile| profile.protein.as_ref())
        .map(ToString::to_string);
    assert_eq!(
        actual_protein.as_deref(),
        expected_protein.as_deref(),
        "protein {source_food_id}"
    );
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
    query("INSERT INTO crumbs_foods_fts (crumbs_foods_fts) VALUES ('integrity-check')")
        .execute(pool)
        .await?;
    query(
        "INSERT INTO crumbs_foods_fts (crumbs_foods_fts, rank)
         VALUES ('integrity-check', 1)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn matching_rowids(
    pool: &SqlitePool,
    fts_query: FtsQuery<'_>,
) -> Result<Vec<RawFoodId>, sqlx::Error> {
    query_scalar("SELECT rowid FROM crumbs_foods_fts WHERE crumbs_foods_fts MATCH ? ORDER BY rowid")
        .bind(fts_query)
        .fetch_all(pool)
        .await
}

fn fts_phrase(text: &str) -> FtsPhrase {
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

async fn applied_migrations(pool: &SqlitePool) -> Result<MigrationCount, sqlx::Error> {
    query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
        .fetch_one(pool)
        .await
}

fn invalid_data(message: impl Into<String>) -> Box<dyn StdError> {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}
