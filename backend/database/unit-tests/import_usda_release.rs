//! Official 2026-10-29 USDA Foundation Foods regression test (CC0 1.0).
//! https://fdc.nal.usda.gov/fdc-datasets/FoodData_Central_foundation_food_json_2026-10-29.zip

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sqlx::{SqlitePool, query, query_scalar};

use crate::{
    Database, DatabaseConfig, Energy, Mass, MassUnit, MeasuredMass, MeasuredQuantity,
    NutritionBasis, ParsedFoodData, TagName,
};

const RELEASE_JSON_ENV: &str = "USDA_FOUNDATION_JSON";
const ENV_HELP: &str = "USDA_FOUNDATION_JSON must point at the official 2026-10-29 Foundation JSON";
const PINNED_RELEASE: ReleaseDate = "2026-10-29";
const FOUNDATION_FOOD_COUNT: FoodCount = 424;
const EXPECTED_SOURCE: SourceRef<'_> = "usda";
const EXPECTED_FOOD_TYPE: FoodTypeRef<'_> = "Foundation";
/// USDA nutrient identifiers, from the Foundation archive, used to verify the
/// stored model against the reference records.
const ENERGY_ATWATER_SPECIFIC: NutrientId = 2048;
const ENERGY_ATWATER_GENERAL: NutrientId = 2047;
const ENERGY_KCAL: NutrientId = 1008;
const ENERGY_KJ: NutrientId = 1062;
const PROTEIN: NutrientId = 1003;
const CARBOHYDRATE_BY_DIFFERENCE: NutrientId = 1005;
const TOTAL_LIPID: NutrientId = 1004;

type FoodId = i64;
type ReleaseDate = &'static str;
type FoodCount = usize;
type SourceRef<'a> = &'a str;
type FoodTypeRef<'a> = &'a str;
type FtsQuery<'a> = &'a str;
type FtsPhrase = String;
type MigrationCount = i64;
type NutrientId = u64;
type Source = String;
type SourceFoodId = String;
type FoodType = String;
type FoodDescription = String;
type FoodCategory = String;
type FoodTags = String;
type ParsedData = String;
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
    parsed_data: ParsedData,
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

    let first_report = crate::import_usda(&database, json_path.as_path()).await?;
    assert_eq!(
        first_report.committed().get(),
        FOUNDATION_FOOD_COUNT,
        "first import count"
    );
    let mut first = read_stored(&database).await?;
    verify_parsed_against_reference(&first, &reference)?;
    let reviewed_tag = TagName::new("Reviewed")?;
    let (reviewed_food_id, reviewed_food_local_id) = match first.first_mut() {
        Some(food) => {
            food.tags = reviewed_tag.as_str().to_owned();
            (crate::FoodId(food.local_id), food.local_id)
        }
        None => return Err(invalid_data("first import returned no foods")),
    };
    database
        .set_food_tags(reviewed_food_id, std::slice::from_ref(&reviewed_tag))
        .await?;
    let tag_matches = matching_rowids(&database.pool, "tags:reviewed").await?;
    assert_eq!(tag_matches, vec![reviewed_food_local_id], "FTS tag");
    verify_fts(&database.pool, &first).await?;
    verify_foreign_keys(&database.pool).await?;
    database.close().await;

    let reopened = Database::open(config(database_path)).await?;
    let applied_after_reopen = applied_migrations(&reopened.pool).await?;
    assert_eq!(applied_after_reopen, applied_before, "reopen migrations");

    let second_report = crate::import_usda(&reopened, json_path.as_path()).await?;
    assert_eq!(
        second_report.committed().get(),
        FOUNDATION_FOOD_COUNT,
        "reimport count"
    );
    let second = read_stored(&reopened).await?;
    assert_eq!(first, second, "reimport changed stored foods");
    verify_fts(&reopened.pool, &second).await?;
    let tag_matches = matching_rowids(&reopened.pool, "tags:reviewed").await?;
    assert_eq!(
        tag_matches,
        vec![reviewed_food_local_id],
        "reopened and reimported FTS tag"
    );
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
    let mut root: Value = serde_json::from_slice(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("parse {}: {error}", path.display()),
        )
    })?;
    let foods = root
        .as_object_mut()
        .and_then(|root| root.get_mut("FoundationFoods"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_data("root must contain a FoundationFoods array"))?;
    if foods.len() != FOUNDATION_FOOD_COUNT {
        return Err(invalid_data(format!(
            "expected {FOUNDATION_FOOD_COUNT} FoundationFoods for {PINNED_RELEASE}, found {}",
            foods.len()
        )));
    }

    let mut reference = BTreeMap::new();
    for food in foods.drain(..) {
        let source_food_id = food
            .get("fdcId")
            .and_then(Value::as_i64)
            .ok_or_else(|| invalid_data("missing fdcId"))?
            .to_string();
        if reference.insert(source_food_id.clone(), food).is_some() {
            return Err(invalid_data(format!("duplicate fdcId {source_food_id}")));
        }
    }
    Ok(reference)
}

async fn read_stored(database: &Database) -> Result<Vec<StoredFood>, sqlx::Error> {
    sqlx::query_as::<_, StoredFood>(
        "SELECT f.id, f.source, f.source_food_id, f.food_type, f.description, f.category,
                s.tags AS tags, f.parsed_data
         FROM crumbs_foods AS f
         JOIN crumbs_foods_search_content AS s ON s.id = f.id
         ORDER BY f.id",
    )
    .fetch_all(&database.pool)
    .await
}

/// Verifies the stored parsed data against the reference archive records.
///
/// Every stored `parsed_data` must deserialize into the shared model, and the
/// typed amounts must match the reference's selected values: energy follows
/// the 2048, 2047, 1008, 1062 priority, protein keeps nutrient 1003,
/// carbohydrate keeps nutrient 1005 including its negative by-difference
/// values, and every reference portion with a positive gram weight becomes
/// one serving with that weight.
fn verify_parsed_against_reference(
    stored: &[StoredFood],
    reference: &ReferenceFoods,
) -> TestResult {
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

        let parsed: ParsedFoodData = serde_json::from_str(&food.parsed_data).map_err(|error| {
            invalid_data(format!("invalid parsed_data {source_food_id}: {error}"))
        })?;
        let profile = match parsed.nutrition.as_ref() {
            Some(profile) => profile,
            None => {
                assert!(
                    reference_energy(expected, source_food_id)?.is_none()
                        && reference_mass(expected, PROTEIN)?.is_none()
                        && reference_mass(expected, CARBOHYDRATE_BY_DIFFERENCE)?.is_none()
                        && reference_mass(expected, TOTAL_LIPID)?.is_none(),
                    "profile is absent for {source_food_id} but the reference reports values"
                );
                continue;
            }
        };
        assert_eq!(
            profile.per,
            NutritionBasis::Per100Grams,
            "basis {source_food_id}"
        );
        assert_eq!(
            profile.energy,
            reference_energy(expected, source_food_id)?,
            "energy {source_food_id}"
        );
        assert_eq!(
            profile.protein,
            reference_mass(expected, PROTEIN)?,
            "protein {source_food_id}"
        );
        assert_eq!(
            profile.carbohydrates.including_fibre,
            reference_mass(expected, CARBOHYDRATE_BY_DIFFERENCE)?,
            "carbohydrate {source_food_id}"
        );
        verify_servings(&parsed, expected, source_food_id)?;
    }
    Ok(())
}

/// Selects the reference energy following the project priority order.
fn reference_energy(
    food: &Value,
    source_food_id: &str,
) -> Result<Option<Energy>, Box<dyn StdError>> {
    for nutrient_id in [
        ENERGY_ATWATER_SPECIFIC,
        ENERGY_ATWATER_GENERAL,
        ENERGY_KCAL,
        ENERGY_KJ,
    ] {
        if let Some((amount, unit)) = reference_nutrient(food, nutrient_id) {
            let amount = crate::measure::decimal_from_json_number(&amount)
                .map_err(|error| invalid_data(format!("energy {source_food_id}: {error}")))?;
            let energy = Energy::from_parts(&unit, amount, None)
                .map_err(|error| invalid_data(format!("energy {source_food_id}: {error}")))?;
            return Ok(Some(energy));
        }
    }
    Ok(None)
}

/// Returns the reference amount of a mass nutrient, when the food reports it.
fn reference_mass(
    food: &Value,
    nutrient_id: NutrientId,
) -> Result<Option<Mass>, Box<dyn StdError>> {
    let Some((amount, unit)) = reference_nutrient(food, nutrient_id) else {
        return Ok(None);
    };
    let amount = crate::measure::decimal_from_json_number(&amount)
        .map_err(|error| invalid_data(format!("nutrient {nutrient_id}: {error}")))?;
    let mass = Mass::from_parts(&unit, amount, None)
        .map_err(|error| invalid_data(format!("nutrient {nutrient_id}: {error}")))?;
    Ok(Some(mass))
}

/// Returns a nutrient entry's exact amount and unit, when it reports one.
fn reference_nutrient(
    food: &Value,
    nutrient_id: NutrientId,
) -> Option<(serde_json::Number, String)> {
    let entry = food
        .get("foodNutrients")?
        .as_array()?
        .iter()
        .find(|entry| {
            entry
                .get("nutrient")
                .and_then(|nutrient| nutrient.get("id"))
                .and_then(Value::as_u64)
                .is_some_and(|id| id == nutrient_id)
        })?;
    let amount = entry.get("amount")?.as_number()?.clone();
    let unit = entry
        .get("nutrient")
        .and_then(|nutrient| nutrient.get("unitName"))
        .and_then(Value::as_str)?
        .to_owned();
    Some((amount, unit))
}

/// Verifies that every reference portion with a positive gram weight became
/// one serving carrying exactly that weight.
fn verify_servings(parsed: &ParsedFoodData, expected: &Value, source_food_id: &str) -> TestResult {
    let portions = match expected.get("foodPortions") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(portions)) => portions.clone(),
        Some(other) => {
            return Err(invalid_data(format!(
                "bad foodPortions for {source_food_id}: {other:?}"
            )));
        }
    };
    let mut expected_weights = Vec::new();
    for portion in &portions {
        let weight = match portion.get("gramWeight").and_then(Value::as_number) {
            Some(weight) => crate::measure::decimal_from_json_number(weight)
                .map_err(|error| invalid_data(format!("gramWeight {source_food_id}: {error}")))?,
            None => continue,
        };
        if weight <= rust_decimal::Decimal::ZERO {
            continue;
        }
        expected_weights.push(weight);
    }
    assert_eq!(
        parsed.serving_sizes.len(),
        expected_weights.len(),
        "serving count {source_food_id}"
    );
    for (serving, weight) in parsed.serving_sizes.iter().zip(expected_weights) {
        let quantity = MeasuredMass::new(MassUnit::Gram, weight)?;
        assert_eq!(
            serving.quantity,
            MeasuredQuantity::Mass(quantity),
            "serving weight {source_food_id}"
        );
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
) -> Result<Vec<FoodId>, sqlx::Error> {
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

fn config(path: PathBuf) -> DatabaseConfig {
    DatabaseConfig { path }
}

fn invalid_data(message: impl Into<String>) -> Box<dyn StdError> {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}
