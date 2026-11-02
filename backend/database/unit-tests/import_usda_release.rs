//! Official 2026-10-29 USDA Foundation Foods regression test (CC0 1.0).
//! https://fdc.nal.usda.gov/fdc-datasets/FoodData_Central_foundation_food_json_2026-10-29.zip

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;
use serde_json::value::RawValue;
use sqlx::{SqlitePool, query, query_scalar};

use crate::{Database, DatabaseConfig, TagName};

const RELEASE_JSON_ENV: &str = "USDA_FOUNDATION_JSON";
const ENV_HELP: &str = "USDA_FOUNDATION_JSON must point at the official 2026-10-29 Foundation JSON";
const PINNED_RELEASE: ReleaseDate = "2026-10-29";
const FOUNDATION_FOOD_COUNT: FoodCount = 424;
const EXPECTED_SOURCE: SourceRef<'_> = "usda";
const EXPECTED_FOOD_TYPE: FoodTypeRef<'_> = "Foundation";

type FoodId = i64;
type ReleaseDate = &'static str;
type FoodCount = usize;
type SourceRef<'a> = &'a str;
type FoodTypeRef<'a> = &'a str;
type FtsQuery<'a> = &'a str;
type FtsPhrase = String;
type MigrationCount = i64;
type Source = String;
type SourceFoodId = String;
type FoodType = String;
type FoodDescription = String;
type FoodCategory = String;
type FoodTags = String;
type ParsedDataJson = String;
type NutrientId = u64;
type ReferenceFoods = BTreeMap<SourceFoodId, SourceFoodReference>;
type TestResult = Result<(), Box<dyn StdError>>;

#[derive(Deserialize)]
struct FoundationArchive {
    #[serde(rename = "FoundationFoods")]
    foundation_foods: Option<Vec<Box<RawValue>>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceFoodReference {
    fdc_id: i64,
    description: String,
    food_category: Option<SourceFoodCategory>,
    #[serde(default)]
    food_nutrients: Vec<SourceNutrient>,
}

#[derive(Deserialize)]
struct SourceFoodCategory {
    description: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceNutrient {
    nutrient: SourceNutrientInfo,
    amount: Option<Box<RawValue>>,
    loq: Option<Box<RawValue>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceNutrientInfo {
    id: Option<NutrientId>,
    unit_name: String,
}

#[derive(Debug, PartialEq)]
struct StoredFood {
    local_id: FoodId,
    source: Source,
    source_food_id: SourceFoodId,
    food_type: FoodType,
    description: FoodDescription,
    category: Option<FoodCategory>,
    tags: FoodTags,
    parsed_data: crate::nutrition::ParsedFoodData,
}

#[derive(sqlx::FromRow)]
struct StoredFoodRow {
    #[sqlx(rename = "id")]
    local_id: FoodId,
    source: Source,
    source_food_id: SourceFoodId,
    food_type: FoodType,
    description: FoodDescription,
    category: Option<FoodCategory>,
    tags: FoodTags,
    parsed_data: ParsedDataJson,
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
    verify_reference(&first, &reference)?;
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
    let archive: FoundationArchive = serde_json::from_slice(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("parse {}: {error}", path.display()),
        )
    })?;
    let foods = archive
        .foundation_foods
        .ok_or_else(|| invalid_data("root must contain a FoundationFoods array"))?;
    if foods.len() != FOUNDATION_FOOD_COUNT {
        return Err(invalid_data(format!(
            "expected {FOUNDATION_FOOD_COUNT} FoundationFoods for {PINNED_RELEASE}, found {}",
            foods.len()
        )));
    }

    let mut reference = BTreeMap::new();
    for food in foods {
        let food: SourceFoodReference = serde_json::from_str(food.get())?;
        let source_food_id = food.fdc_id.to_string();
        if reference.insert(source_food_id.clone(), food).is_some() {
            return Err(invalid_data(format!("duplicate fdcId {source_food_id}")));
        }
    }
    Ok(reference)
}

async fn read_stored(database: &Database) -> Result<Vec<StoredFood>, Box<dyn StdError>> {
    let rows = sqlx::query_as::<_, StoredFoodRow>(
        "SELECT f.id, f.source, f.source_food_id, f.food_type, f.description, f.category,
                s.tags AS tags, f.parsed_data
         FROM crumbs_foods AS f
         JOIN crumbs_foods_search_content AS s ON s.id = f.id
         ORDER BY f.id",
    )
    .fetch_all(&database.pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let parsed_data: crate::nutrition::ParsedFoodData =
                serde_json::from_str(&row.parsed_data).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid parsed_data {}: {error}", row.source_food_id),
                    )
                })?;
            Ok(StoredFood {
                local_id: row.local_id,
                source: row.source,
                source_food_id: row.source_food_id,
                food_type: row.food_type,
                description: row.description,
                category: row.category,
                tags: row.tags,
                parsed_data,
            })
        })
        .collect()
}

fn verify_reference(stored: &[StoredFood], reference: &ReferenceFoods) -> TestResult {
    assert_eq!(stored.len(), FOUNDATION_FOOD_COUNT, "row count");
    let mut checked_nutrients = 0;
    for food in stored {
        let source_food_id = food.source_food_id.as_str();
        let Some(expected) = reference.get(source_food_id) else {
            return Err(invalid_data(format!("unknown {source_food_id}")));
        };
        assert_eq!(food.source, EXPECTED_SOURCE, "source {source_food_id}");
        assert_eq!(food.food_type, EXPECTED_FOOD_TYPE, "type {source_food_id}");
        assert_eq!(
            food.description.as_str(),
            expected.description.as_str(),
            "description {source_food_id}"
        );
        assert_eq!(
            food.category.as_deref(),
            expected
                .food_category
                .as_ref()
                .map(|category| category.description.as_str()),
            "category {source_food_id}"
        );
        assert!(food.tags.is_empty(), "tags {source_food_id}");
        checked_nutrients += verify_nutrition(&food.parsed_data, expected, source_food_id)?;
    }
    assert!(checked_nutrients > 0, "no source nutrients were verified");
    Ok(())
}

fn verify_nutrition(
    parsed_data: &crate::nutrition::ParsedFoodData,
    source: &SourceFoodReference,
    source_food_id: &str,
) -> Result<usize, Box<dyn StdError>> {
    use crate::nutrition::{Mass, NutritionBasis};

    let expected_fat = source_reported::<Mass>(source, 1004)?;
    let expected_carbohydrates = source_reported::<Mass>(source, 1005)?;
    let expected_protein = source_reported::<Mass>(source, 1003)?;
    let expected_total_sugars = source_preferred_reported::<Mass>(source, &[2000, 1063])?;
    let expected_energy = source_energy(source)?;
    let profile = parsed_data.nutrition.as_ref();
    if let Some(profile) = profile {
        assert_eq!(
            profile.per,
            NutritionBasis::Per100Grams,
            "nutrition basis {source_food_id}"
        );
    }

    let mut checked = 0;
    if expected_fat.is_some() {
        assert_eq!(
            profile.and_then(|profile| profile.fat.total.as_ref()),
            expected_fat.as_ref(),
            "fat.total {source_food_id}"
        );
        checked += 1;
    }
    if expected_carbohydrates.is_some() {
        assert_eq!(
            profile.and_then(|profile| profile.carbohydrates.including_fibre.as_ref()),
            expected_carbohydrates.as_ref(),
            "carbohydrates.including_fibre {source_food_id}"
        );
        checked += 1;
    }
    if expected_protein.is_some() {
        assert_eq!(
            profile.and_then(|profile| profile.protein.as_ref()),
            expected_protein.as_ref(),
            "protein {source_food_id}"
        );
        checked += 1;
    }
    if expected_total_sugars.is_some() {
        assert_eq!(
            profile.and_then(|profile| profile.carbohydrates.sugars.total.as_ref()),
            expected_total_sugars.as_ref(),
            "carbohydrates.sugars.total {source_food_id}"
        );
        checked += 1;
    }
    if expected_energy.is_some() {
        assert_eq!(
            profile.and_then(|profile| profile.energy.as_ref()),
            expected_energy.as_ref(),
            "energy {source_food_id}"
        );
        checked += 1;
    }
    Ok(checked)
}

fn source_reported<T>(
    source: &SourceFoodReference,
    nutrient_id: NutrientId,
) -> Result<Option<crate::nutrition::Reported<T>>, Box<dyn StdError>>
where
    T: FromStr<Err = crate::nutrition::MeasurementError>,
{
    let Some(nutrient) = source
        .food_nutrients
        .iter()
        .find(|nutrient| nutrient.nutrient.id == Some(nutrient_id) && nutrient.amount.is_some())
    else {
        return Ok(None);
    };
    Ok(Some(source_reported_nutrient(nutrient)?))
}

fn source_preferred_reported<T>(
    source: &SourceFoodReference,
    nutrient_ids: &[NutrientId],
) -> Result<Option<crate::nutrition::Reported<T>>, Box<dyn StdError>>
where
    T: FromStr<Err = crate::nutrition::MeasurementError>,
{
    for nutrient_id in nutrient_ids {
        let Some(nutrient) = source.food_nutrients.iter().find(|nutrient| {
            nutrient.nutrient.id == Some(*nutrient_id) && nutrient.amount.is_some()
        }) else {
            continue;
        };
        return Ok(Some(source_reported_nutrient(nutrient)?));
    }
    Ok(None)
}

fn source_energy(
    source: &SourceFoodReference,
) -> Result<Option<crate::nutrition::Reported<crate::nutrition::Energy>>, Box<dyn StdError>> {
    for nutrient_id in [2048, 2047, 1008, 1062] {
        let Some(nutrient) = source.food_nutrients.iter().find(|nutrient| {
            nutrient.nutrient.id == Some(nutrient_id) && nutrient.amount.is_some()
        }) else {
            continue;
        };
        return Ok(Some(source_reported_nutrient(nutrient)?));
    }
    Ok(None)
}

fn source_reported_nutrient<T>(
    nutrient: &SourceNutrient,
) -> Result<crate::nutrition::Reported<T>, Box<dyn StdError>>
where
    T: FromStr<Err = crate::nutrition::MeasurementError>,
{
    use crate::nutrition::Decimal;

    let amount = nutrient
        .amount
        .as_deref()
        .ok_or_else(|| invalid_data("source nutrient has no amount"))?;
    let amount = source_text(amount)?;
    let unit = canonical_source_unit(&nutrient.nutrient.unit_name);
    let value_and_unit = match amount.parse::<Decimal>() {
        Ok(value) if value.is_zero() => match nutrient.loq.as_deref() {
            Some(loq) => {
                let threshold = source_text(loq)?.parse::<Decimal>()?;
                if threshold.is_zero() {
                    format!("{amount} {unit}")
                } else {
                    format!("< {threshold} {unit}")
                }
            }
            None => format!("{amount} {unit}"),
        },
        _ => format!("{amount} {unit}"),
    };
    Ok(value_and_unit.parse()?)
}

fn source_text(raw: &RawValue) -> Result<String, serde_json::Error> {
    let value = raw.get().trim();
    if value.starts_with('"') {
        serde_json::from_str(value)
    } else {
        Ok(value.to_owned())
    }
}

fn canonical_source_unit(unit: &str) -> &str {
    match unit.to_ascii_lowercase().as_str() {
        "g" => "g",
        "mg" => "mg",
        "ug" | "µg" => "µg",
        "kcal" => "kcal",
        "kj" => "kJ",
        _ => unit,
    }
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
