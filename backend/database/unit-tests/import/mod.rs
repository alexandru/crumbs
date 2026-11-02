use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use sqlx::{FromRow, query, query_scalar};

use super::ImportError;
use crate::nutrition::{Fat, FoodData, NutritionBasis, Sugars};
use crate::{
    Database, DatabaseConfig, Food, FoodCategory, FoodDescription, FoodId, FoodWriter,
    SourceFoodId, TagName, import_usda,
};

const FOUNDATION_JSON: &str = include_str!("../../test-fixtures/foundation.json");
const BRANDED_JSON: &str = include_str!("../../test-fixtures/branded.json");
const IMPORT_TIMEOUT: Duration = Duration::from_secs(5);
const LATER_TRIGGER_ROWS: usize = 100;

#[tokio::test]
async fn persists_survey_wweia_category() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let record = r#"{"fdcId":2705383,"description":"Milk, human","dataType":"Survey (FNDDS)","wweiaFoodCategory":{"wweiaFoodCategoryDescription":"Human milk","wweiaFoodCategoryCode":3298314}}"#;
    let input = format!(r#"{{"SurveyFoods":[{record}]}}"#);
    let path = write_input(directory.path(), "survey.json", &input)?;

    let report = import_usda(&database, &path).await?;

    assert_eq!(report.committed().get(), 1);
    let food = food_row(&database, "usda", "2705383").await?;
    assert_eq!(food.category.as_deref(), Some("Human milk"));
    assert_eq!(food.food_data()?, FoodData::default());
    Ok(())
}

#[tokio::test]
async fn imports_foundation_foods_with_typed_nutrition_and_servings()
-> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), "foundation.json", FOUNDATION_JSON)?;

    let report = import_usda(&database, &path).await?;

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
    let parsed_data = food.food_data()?;
    let profile = parsed_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("Foundation nutrition was not persisted"))?;
    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(
        profile.energy.as_ref().map(ToString::to_string).as_deref(),
        Some("166 kcal")
    );
    assert_eq!(
        profile.protein.as_ref().map(ToString::to_string).as_deref(),
        Some("7.90 g")
    );
    assert_eq!(
        profile
            .fat
            .total
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("9.60 g")
    );
    assert_eq!(
        profile
            .carbohydrates
            .including_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("14.29 g")
    );
    assert_eq!(profile.carbohydrates.excluding_fibre, None);
    assert_eq!(
        profile
            .carbohydrates
            .sugars
            .total
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("0.0 g")
    );
    assert_eq!(profile.carbohydrates.sugars.lactose, None);
    assert_eq!(
        profile.fibre.as_ref().map(ToString::to_string).as_deref(),
        Some("6.0 g")
    );
    assert_eq!(
        profile.sodium.as_ref().map(ToString::to_string).as_deref(),
        Some("379 mg")
    );
    assert_eq!(parsed_data.serving_sizes.len(), 2);
    let serving_quantities: Vec<String> = parsed_data
        .serving_sizes
        .iter()
        .map(|serving| serving.quantity.to_string())
        .collect();
    assert_eq!(
        serving_quantities,
        vec!["33.9 g".to_owned(), "16.95 g".to_owned()]
    );
    let stored_data: serde_json::Value = serde_json::from_str(&food.parsed_data)?;
    let stored_servings = stored_data
        .get("serving_sizes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| io::Error::other("serialized serving definitions were not persisted"))?;
    for serving in stored_servings {
        let fields = serving
            .as_object()
            .ok_or_else(|| io::Error::other("serialized serving definition is not an object"))?;
        assert_eq!(fields.len(), 2);
        assert!(fields.contains_key("name"));
        assert!(fields.contains_key("quantity"));
    }

    let tomato = food_row(&database, "usda", "321360").await?;
    let tomato_data = tomato.food_data()?;
    assert_eq!(tomato_data.nutrition, None);
    assert_eq!(tomato_data.serving_sizes.len(), 1);
    let tomato_serving = tomato_data
        .serving_sizes
        .first()
        .ok_or_else(|| io::Error::other("Tomato serving was not persisted"))?;
    assert_eq!(tomato_serving.quantity.to_string(), "49.7 g");
    Ok(())
}

#[tokio::test]
async fn persists_negative_foundation_carbohydrate_without_defaulting_other_nutrients()
-> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let input = r#"{"FoundationFoods":[{"fdcId":2727566,"description":"Chicken, drumstick, meat and skin, raw","dataType":"Foundation","foodCategory":{"description":"Poultry Products"},"foodNutrients":[{"nutrient":{"id":1005,"unitName":"g"},"amount":-0.475}]}]}"#;
    let path = write_input(directory.path(), "negative-carbohydrate.json", input)?;

    let report = import_usda(&database, &path).await?;

    assert_eq!(report.committed().get(), 1);
    let food = food_row(&database, "usda", "2727566").await?;
    assert_eq!(food.source, "usda");
    assert_eq!(food.food_type, "Foundation");
    assert_eq!(food.description, "Chicken, drumstick, meat and skin, raw");
    assert_eq!(food.category.as_deref(), Some("Poultry Products"));
    let parsed_data = food.food_data()?;
    assert!(parsed_data.serving_sizes.is_empty());
    let profile = parsed_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("negative carbohydrate profile was not persisted"))?;
    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(
        profile
            .carbohydrates
            .including_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("-0.475 g")
    );
    assert_eq!(profile.energy, None);
    assert_eq!(profile.fat, Fat::default());
    assert_eq!(profile.carbohydrates.excluding_fibre, None);
    assert_eq!(profile.carbohydrates.sugars, Sugars::default());
    assert_eq!(profile.carbohydrates.starch, None);
    assert_eq!(profile.fibre, None);
    assert_eq!(profile.protein, None);
    assert_eq!(profile.sodium, None);
    assert_eq!(profile.salt, None);
    assert_eq!(profile.cholesterol, None);
    Ok(())
}

#[tokio::test]
async fn imports_branded_food_with_serving_and_no_unstandardized_nutrition()
-> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), "branded.json", BRANDED_JSON)?;

    let report = import_usda(&database, &path).await?;

    assert_eq!(report.committed().get(), 1);
    let food = food_row(&database, "usda", "1106281").await?;
    assert_eq!(food.source, "usda");
    assert_eq!(food.food_type, "Branded");
    assert_eq!(food.description, "GRANOLA, CINNAMON, RAISIN");
    assert_eq!(food.category.as_deref(), Some("Cereal"));
    assert!(database.list_tags().await?.is_empty());
    let parsed_data = food.food_data()?;
    assert_eq!(parsed_data.nutrition, None);
    let serving = parsed_data
        .serving_sizes
        .first()
        .ok_or_else(|| io::Error::other("Branded serving was not persisted"))?;
    assert_eq!(serving.quantity.to_string(), "28 g");
    Ok(())
}

#[tokio::test]
async fn reimport_updates_in_place_and_retains_internal_id() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), "foundation.json", FOUNDATION_JSON)?;
    import_usda(&database, &path).await?;
    let id_before = food_row(&database, "usda", "321358").await?.id;

    query(
        "UPDATE crumbs_foods
         SET description = 'stale', category = NULL, parsed_data = '{\"nutrition\":null,\"serving_sizes\":[]}'
         WHERE source = 'usda' AND source_food_id = '321358'",
    )
    .execute(&database.pool)
    .await?;

    let report = import_usda(&database, &path).await?;

    assert_eq!(report.committed().get(), 2);
    assert_eq!(food_count(&database).await?, 2);
    let food = food_row(&database, "usda", "321358").await?;
    assert_eq!(food.id, id_before, "upsert replaced the internal row id");
    assert_eq!(food.description, "Hummus, commercial");
    assert_eq!(
        food.category.as_deref(),
        Some("Legumes and Legume Products")
    );
    let parsed_data = food.food_data()?;
    let profile = parsed_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("reimported Foundation nutrition was not persisted"))?;
    assert_eq!(
        profile.protein.as_ref().map(ToString::to_string).as_deref(),
        Some("7.90 g")
    );
    Ok(())
}

#[tokio::test]
async fn cross_source_same_food_id_is_isolated() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), "foundation.json", FOUNDATION_JSON)?;
    import_usda(&database, &path).await?;
    query("INSERT INTO crumbs_sources (id, created_at) VALUES ('other', 0)")
        .execute(&database.pool)
        .await?;
    let other_food_id: RowId = query_scalar(
        "INSERT INTO crumbs_foods (
            source, source_food_id, food_type, description, category, parsed_data, created_at
         ) VALUES ('other', '321358', 'Foundation', 'Other source food', NULL,
                   '{\"nutrition\":null,\"serving_sizes\":[]}', 0)
         RETURNING id",
    )
    .fetch_one(&database.pool)
    .await?;
    let other_tag = TagName::new("Other")?;
    database
        .set_food_tags(FoodId(other_food_id), std::slice::from_ref(&other_tag))
        .await?;

    import_usda(&database, &path).await?;

    let other = food_row(&database, "other", "321358").await?;
    assert_eq!(other.source, "other");
    assert_eq!(other.description, "Other source food");
    assert_eq!(other.food_data()?, FoodData::default());
    let tagged_foods: Vec<RowId> = query_scalar(
        "SELECT ft.food_id FROM crumbs_food_tags ft
         JOIN crumbs_tags t ON t.id = ft.tag_id
         WHERE t.name = ? ORDER BY ft.food_id",
    )
    .bind(other_tag.as_str())
    .fetch_all(&database.pool)
    .await?;
    assert_eq!(tagged_foods, vec![other_food_id]);
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

    import_usda(&database, &foundation).await?;
    let report = import_usda(&database, &branded).await?;

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
        r#"{"SRLegacyFoods":[{"fdcId":9301,"description":"Legacy food","dataType":"SR Legacy","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":10.25}]}]}"#,
    )?;
    let survey = write_input(
        directory.path(),
        "survey.json",
        r#"{"SurveyFoods":[{"fdcId":9302,"description":"Survey food","dataType":"Survey (FNDDS)","foodNutrients":[{"nutrient":{"id":1005,"unitName":"g"},"amount":8.5}]}]}"#,
    )?;

    import_usda(&database, &sr).await?;
    import_usda(&database, &survey).await?;

    let sr_food = food_row(&database, "usda", "9301").await?;
    let survey_food = food_row(&database, "usda", "9302").await?;
    assert_eq!(sr_food.food_type, "SR Legacy");
    assert_eq!(survey_food.food_type, "Survey (FNDDS)");
    let sr_profile = sr_food
        .food_data()?
        .nutrition
        .ok_or_else(|| io::Error::other("SR Legacy nutrition was not persisted"))?;
    assert_eq!(sr_profile.per, NutritionBasis::Per100Grams);
    assert_eq!(
        sr_profile
            .protein
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("10.25 g")
    );
    let survey_profile = survey_food
        .food_data()?
        .nutrition
        .ok_or_else(|| io::Error::other("Survey nutrition was not persisted"))?;
    assert_eq!(survey_profile.per, NutritionBasis::Per100Grams);
    assert_eq!(
        survey_profile
            .carbohydrates
            .including_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("8.5 g")
    );
    Ok(())
}

#[tokio::test]
async fn branded_volume_nutrition_keeps_millilitres_and_serving_quantity()
-> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let record = r#"{"fdcId":9303,"description":"Milk drink","dataType":"Branded","brandedFoodCategory":"Dairy","servingSize":250,"servingSizeUnit":"ml","householdServingFullText":"one bottle","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":3.25}]}"#;
    let input = format!(r#"{{"BrandedFoods":[{record}]}}"#);
    let path = write_input(directory.path(), "branded-volume.json", &input)?;

    let report = import_usda(&database, &path).await?;

    assert_eq!(report.committed().get(), 1);
    let food = food_row(&database, "usda", "9303").await?;
    let parsed_data = food.food_data()?;
    let profile = parsed_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("Branded volume nutrition was not persisted"))?;
    assert_eq!(profile.per, NutritionBasis::Per100Millilitres);
    assert_eq!(
        profile.protein.as_ref().map(ToString::to_string).as_deref(),
        Some("3.25 g")
    );
    let serving = parsed_data
        .serving_sizes
        .first()
        .ok_or_else(|| io::Error::other("Branded volume serving was not persisted"))?;
    assert_eq!(serving.quantity.to_string(), "250 ml");
    Ok(())
}

#[tokio::test]
async fn empty_recognized_array_succeeds_with_zero_commits() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), "empty.json", r#"{"FoundationFoods":[]}"#)?;

    let report = import_usda(&database, &path).await?;

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
    let report = import_usda(&database, &path).await?;

    assert_eq!(report.committed().get(), 2);
    assert_eq!(food_count(&database).await?, 2);
    food_row(&database, "usda", "9102").await?;
    Ok(())
}

#[tokio::test]
async fn invalid_selected_nutrient_after_valid_food_reports_committed_progress()
-> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(
        directory.path(),
        "invalid-nutrient.json",
        r#"{"FoundationFoods":[
            {"fdcId":9151,"description":"First food","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1.0}]},
            {"fdcId":9152,"description":"Invalid energy","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":2048,"unitName":"kcal"},"amount":"not a decimal"},{"nutrient":{"id":2047,"unitName":"kcal"},"amount":100}]}
        ]}"#,
    )?;

    let error = require_import_error(&database, &path).await?;

    assert!(error.to_string().contains("2048"), "{error}");
    assert!(error.to_string().contains("record 2"), "{error}");
    assert!(error.to_string().contains("committed 1 foods"), "{error}");
    assert_eq!(food_count(&database).await?, 1);
    let first = food_row(&database, "usda", "9151").await?;
    assert!(first.food_data()?.nutrition.is_some());
    assert_row_absent(&database, "usda", "9152").await?;
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
    let report = import_usda(&database, &path).await?;

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
async fn generic_food_writer_receives_typed_parsed_data() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let first_record = r#"{"fdcId":7301, "description":"Alternative food", "dataType":"Foundation", "foodCategory":{"description":"Shelf Stable"}, "foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1.00}]}"#;
    let first_input = format!(r#"{{"FoundationFoods":[{first_record}]}}"#);
    let first_path = write_input(directory.path(), "first.json", &first_input)?;
    let store = RecordingFoodWriter::default();

    let report = import_usda(&store, &first_path).await?;

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
    let first_profile = first_food
        .parsed_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("recording writer did not receive nutrition"))?;
    assert_eq!(
        first_profile
            .protein
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("1.00 g")
    );

    let second_record = r#"{"fdcId":7301, "description":"Updated alternative food", "dataType":"Foundation", "foodCategory":{"description":"Shelf Stable"}, "foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":2.00}]}"#;
    let second_input = format!(r#"{{"FoundationFoods":[{second_record}]}}"#);
    let second_path = write_input(directory.path(), "second.json", &second_input)?;
    let report = import_usda(&store, &second_path).await?;

    assert_eq!(report.committed().get(), 1);
    let updated_food = store
        .snapshot()?
        .into_iter()
        .find(|food| food.source_food_id == "7301")
        .ok_or_else(|| io::Error::other("recording store lost the food"))?;
    assert_eq!(updated_food.id, first_food.id);
    assert_eq!(updated_food.description, "Updated alternative food");
    let updated_profile = updated_food
        .parsed_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("recording writer lost updated nutrition"))?;
    assert_eq!(
        updated_profile
            .protein
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("2.00 g")
    );
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
    let result = tokio::time::timeout(IMPORT_TIMEOUT, import_usda(database, path))
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
    let result = tokio::time::timeout(IMPORT_TIMEOUT, import_usda(store, path))
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

async fn food_count(database: &Database) -> Result<FoodCount, sqlx::Error> {
    query_scalar("SELECT COUNT(*) FROM crumbs_foods")
        .fetch_one(&database.pool)
        .await
}

type SourceText = String;
type FoodTypeText = String;
type ParsedDataText = String;
type RowId = i64;
type FoodCount = i64;
type WriteCount = usize;

#[derive(Debug, FromRow)]
struct FoodRow {
    id: RowId,
    source: SourceText,
    food_type: FoodTypeText,
    description: FoodDescription,
    category: Option<FoodCategory>,
    parsed_data: ParsedDataText,
}

impl FoodRow {
    fn food_data(&self) -> Result<FoodData, serde_json::Error> {
        serde_json::from_str(&self.parsed_data)
    }
}

async fn food_row(
    database: &Database,
    source: &str,
    source_food_id: &str,
) -> Result<FoodRow, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, source, food_type, description, category, parsed_data
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
    fail_after: Option<WriteCount>,
}

#[derive(Default)]
struct RecordingFoodState {
    foods: HashMap<(SourceText, SourceFoodId), RecordedFood>,
    last_id: RowId,
    successful_writes: WriteCount,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedFood {
    id: FoodId,
    source: SourceText,
    source_food_id: SourceFoodId,
    food_type: FoodTypeText,
    description: FoodDescription,
    category: Option<FoodCategory>,
    parsed_data: FoodData,
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
            parsed_data: food.parsed_data.clone(),
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
