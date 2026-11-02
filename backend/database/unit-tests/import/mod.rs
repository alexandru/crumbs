use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use sqlx::{query, query_scalar};

use super::ImportError;
use crate::nutrition::{Energy, Mass, NutritionBasis, ParsedFoodData, Reported};
use crate::{
    Database, DatabaseConfig, Food, FoodCategory, FoodDescription, FoodId, FoodWriter,
    SourceFoodId, TagName, import_usda,
};

const FOUNDATION_JSON: &str = include_str!("../../test-fixtures/foundation.json");
const BRANDED_JSON: &str = include_str!("../../test-fixtures/branded.json");
const STANDARD_NUTRIENTS: &str = r#"[
    {"nutrient":{"id":1003,"unitName":"g"},"amount":7.000000000000000001},
    {"nutrient":{"id":1005,"unitName":"g"},"amount":28.5},
    {"nutrient":{"id":1079,"unitName":"g"},"amount":4},
    {"nutrient":{"id":1004,"unitName":"g"},"amount":0},
    {"nutrient":{"id":1093,"unitName":"mg"},"amount":150}
]"#;
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
    assert_eq!(food.parsed_data.nutrition, None);
    assert!(food.parsed_data.serving_sizes.is_empty());
    Ok(())
}

#[tokio::test]
async fn imports_foundation_foods_and_parses_supported_serving_sizes()
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
    assert_foundation_hummus_nutrition(&food)?;
    let serving = food
        .parsed_data
        .serving_sizes
        .first()
        .ok_or_else(|| io::Error::other("foundation food has no parsed serving size"))?;
    assert_eq!(serving.name.to_string(), "2 tablespoon");
    assert_eq!(serving.quantity.to_string(), "33.9 g");
    Ok(())
}

#[tokio::test]
async fn imports_branded_food_metadata() -> Result<(), Box<dyn StdError>> {
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
         SET description = 'stale', category = NULL, parsed_data = '{\"stale\":true}'
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
    assert_foundation_hummus_nutrition(&food)?;
    let serving = food
        .parsed_data
        .serving_sizes
        .first()
        .ok_or_else(|| io::Error::other("reimported food has no parsed serving size"))?;
    assert_eq!(serving.quantity.to_string(), "33.9 g");
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
         ) VALUES ('other', '321358', 'Foundation', 'Other source food', NULL, '{\"nutrition\":null,\"serving_sizes\":[]}', 0)
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
    assert_eq!(other.parsed_data, ParsedFoodData::default());
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
        r#"{"SRLegacyFoods":[{"fdcId":9301,"description":"Legacy food","dataType":"SR Legacy"}]}"#,
    )?;
    let survey = write_input(
        directory.path(),
        "survey.json",
        r#"{"SurveyFoods":[{"fdcId":9302,"description":"Survey food","dataType":"Survey (FNDDS)"}]}"#,
    )?;

    import_usda(&database, &sr).await?;
    import_usda(&database, &survey).await?;

    let sr_food = food_row(&database, "usda", "9301").await?;
    let survey_food = food_row(&database, "usda", "9302").await?;
    assert_eq!(sr_food.food_type, "SR Legacy");
    assert_eq!(survey_food.food_type, "Survey (FNDDS)");
    Ok(())
}

#[tokio::test]
async fn persists_typed_nutrition_from_each_supported_root() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let roots = [
        ("sr.json", "SRLegacyFoods", "SR Legacy", "9401", false),
        (
            "survey.json",
            "SurveyFoods",
            "Survey (FNDDS)",
            "9402",
            false,
        ),
        (
            "foundation.json",
            "FoundationFoods",
            "Foundation",
            "9403",
            false,
        ),
        ("branded.json", "BrandedFoods", "Branded", "9404", true),
    ];

    for (filename, root, food_type, fdc_id, branded) in roots {
        let serving_fields = if branded {
            r#""servingSizeUnit":"g","servingSize":100,"#
        } else {
            ""
        };
        let label_fields = if branded {
            r#","labelNutrients":{"protein":{"value":7.000000000000000001},"carbohydrates":{"value":28.5},"fiber":{"value":4},"fat":{"value":0},"sodium":{"value":150}}"#
        } else {
            ""
        };
        let input = format!(
            r#"{{"{root}":[{{"fdcId":{fdc_id},"description":"Standardized food","dataType":"{food_type}",{serving_fields}"foodNutrients":{STANDARD_NUTRIENTS}{label_fields}}}]}}"#
        );
        let path = write_input(directory.path(), filename, &input)?;

        let report = import_usda(&database, &path).await?;

        assert_eq!(report.committed().get(), 1, "{root}");
        let food = food_row(&database, "usda", fdc_id).await?;
        assert_standard_nutrition(&food)?;
    }

    Ok(())
}

#[tokio::test]
async fn preserves_measured_portions_when_nutrition_is_absent() -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let input = r#"{"FoundationFoods":[{"fdcId":9501,"description":"Sliced food","dataType":"Foundation","foodPortions":[{"amount":1,"measureUnit":{"name":"slice"},"gramWeight":30},{"amount":2,"measureUnit":{"name":"slice"},"gramWeight":60}]}]}"#;
    let path = write_input(directory.path(), "portions.json", input)?;

    let report = import_usda(&database, &path).await?;

    assert_eq!(report.committed().get(), 1);
    let food = food_row(&database, "usda", "9501").await?;
    assert_eq!(food.parsed_data.nutrition, None);
    assert_eq!(food.parsed_data.serving_sizes.len(), 2);
    let first = food
        .parsed_data
        .serving_sizes
        .first()
        .ok_or_else(|| io::Error::other("first measured portion was not persisted"))?;
    assert_eq!(first.name.to_string(), "1 slice");
    assert_eq!(first.quantity.to_string(), "30 g");
    let second = food
        .parsed_data
        .serving_sizes
        .get(1)
        .ok_or_else(|| io::Error::other("second measured portion was not persisted"))?;
    assert_eq!(second.name.to_string(), "2 slice");
    assert_eq!(second.quantity.to_string(), "60 g");
    Ok(())
}

#[tokio::test]
async fn invalid_supported_nutrient_reports_record_and_keeps_prior_commit()
-> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let input = r#"{"FoundationFoods":[
        {"fdcId":9601,"description":"Valid protein","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":5}]},
        {"fdcId":9602,"description":"Negative protein","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":-1}]}
    ]}"#;
    let path = write_input(directory.path(), "invalid-protein.json", input)?;

    let error = require_import_error(&database, &path).await?;

    assert!(error.to_string().contains("record 2"), "{error}");
    assert!(error.to_string().contains("committed 1 foods"), "{error}");
    assert_eq!(food_count(&database).await?, 1);
    let first = food_row(&database, "usda", "9601").await?;
    let profile = first
        .parsed_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("valid protein profile was not persisted"))?;
    let expected_protein: Reported<Mass> = "5 g".parse()?;
    assert_eq!(profile.protein.as_ref(), Some(&expected_protein));
    assert_row_absent(&database, "usda", "9602").await?;
    Ok(())
}

#[tokio::test]
async fn energy_candidates_use_identifier_priority_and_fall_through_missing_values()
-> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let input = r#"{"FoundationFoods":[
        {"fdcId":9701,"description":"All energy candidates","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":1062,"unitName":"kJ"},"amount":420},{"nutrient":{"id":1008,"unitName":"kcal"},"amount":100},{"nutrient":{"id":2047,"unitName":"kcal"},"amount":90},{"nutrient":{"id":2048,"unitName":"kcal"},"amount":80}]},
        {"fdcId":9702,"description":"Second energy candidate","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":1062,"unitName":"kJ"},"amount":420},{"nutrient":{"id":1008,"unitName":"kcal"},"amount":100},{"nutrient":{"id":2047,"unitName":"kcal"},"amount":90}]},
        {"fdcId":9703,"description":"Third energy candidate","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":1062,"unitName":"kJ"},"amount":420},{"nutrient":{"id":1008,"unitName":"kcal"},"amount":100}]},
        {"fdcId":9704,"description":"Kilojoule energy","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":1062,"unitName":"kJ"},"amount":420}]},
        {"fdcId":9705,"description":"Missing preferred amount","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":1062,"unitName":"kJ"},"amount":420},{"nutrient":{"id":2047,"unitName":"kcal"},"amount":90},{"nutrient":{"id":2048,"unitName":"kcal"}}]},
        {"fdcId":9706,"description":"No energy","dataType":"Foundation","foodNutrients":[]}
    ]}"#;
    let path = write_input(directory.path(), "energy-priority.json", input)?;

    let report = import_usda(&database, &path).await?;

    assert_eq!(report.committed().get(), 6);
    for (fdc_id, expected) in [
        ("9701", Some("80 kcal")),
        ("9702", Some("90 kcal")),
        ("9703", Some("100 kcal")),
        ("9704", Some("420 kJ")),
        ("9705", Some("90 kcal")),
        ("9706", None),
    ] {
        let food = food_row(&database, "usda", fdc_id).await?;
        assert_energy(&food, expected)?;
    }
    Ok(())
}

#[tokio::test]
async fn invalid_preferred_energy_does_not_use_a_valid_fallback() -> Result<(), Box<dyn StdError>> {
    let input = r#"{"FoundationFoods":[
        {"fdcId":9801,"description":"Prior commit","dataType":"Foundation"},
        {"fdcId":9802,"description":"Negative preferred energy","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":2047,"unitName":"kcal"},"amount":100},{"nutrient":{"id":2048,"unitName":"kcal"},"amount":-1}]}
    ]}"#;
    assert_invalid_energy_record_two(input, "negative-energy.json", "9801", "9802").await
}

#[tokio::test]
async fn wrong_unit_for_preferred_energy_does_not_use_a_valid_fallback()
-> Result<(), Box<dyn StdError>> {
    let input = r#"{"FoundationFoods":[
        {"fdcId":9811,"description":"Prior commit","dataType":"Foundation"},
        {"fdcId":9812,"description":"Wrong-unit preferred energy","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":2047,"unitName":"kcal"},"amount":100},{"nutrient":{"id":2048,"unitName":"kJ"},"amount":400}]}
    ]}"#;
    assert_invalid_energy_record_two(input, "wrong-unit-energy.json", "9811", "9812").await
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
async fn imports_into_generic_food_writer_and_preserves_typed_parsed_data()
-> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let first_record = r#"{"fdcId":7301, "description":"Alternative food", "dataType":"Foundation", "foodCategory":{"description":"Shelf Stable"}, "details":{"measurement":1.00}, "foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1}]}"#;
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
    let first_protein: Reported<Mass> = "1 g".parse()?;
    let first_profile = first_food
        .parsed_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("recording store received no nutrition profile"))?;
    assert_eq!(first_profile.protein.as_ref(), Some(&first_protein));

    let second_record = r#"{"fdcId":7301, "description":"Updated alternative food", "dataType":"Foundation", "foodCategory":{"description":"Shelf Stable"}, "details":{"measurement":2.00}, "foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":2}]}"#;
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
    let updated_protein: Reported<Mass> = "2 g".parse()?;
    let updated_profile =
        updated_food.parsed_data.nutrition.as_ref().ok_or_else(|| {
            io::Error::other("recording store lost the updated nutrition profile")
        })?;
    assert_eq!(updated_profile.protein.as_ref(), Some(&updated_protein));
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

fn assert_standard_nutrition(food: &FoodRow) -> Result<(), Box<dyn StdError>> {
    let profile = food
        .parsed_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("standardized nutrients produced no profile"))?;
    let expected_protein: Reported<Mass> = "7.000000000000000001 g".parse()?;
    let expected_carbohydrates: Reported<Mass> = "28.5 g".parse()?;
    let expected_fibre: Reported<Mass> = "4 g".parse()?;
    let expected_zero_fat: Reported<Mass> = "0 g".parse()?;
    let expected_sodium: Reported<Mass> = "150 mg".parse()?;

    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(profile.protein.as_ref(), Some(&expected_protein));
    assert_eq!(
        profile.carbohydrates.including_fibre.as_ref(),
        Some(&expected_carbohydrates)
    );
    assert_eq!(profile.carbohydrates.excluding_fibre.as_ref(), None);
    assert_eq!(profile.fibre.as_ref(), Some(&expected_fibre));
    assert_eq!(profile.fat.total.as_ref(), Some(&expected_zero_fat));
    assert_eq!(profile.fat.saturated.as_ref(), None);
    assert_eq!(profile.fat.monounsaturated.as_ref(), None);
    assert_eq!(profile.fat.polyunsaturated.as_ref(), None);
    assert_eq!(profile.fat.trans.as_ref(), None);
    assert_eq!(profile.carbohydrates.sugars.lactose.as_ref(), None);
    assert_eq!(profile.sodium.as_ref(), Some(&expected_sodium));
    assert_eq!(profile.salt.as_ref(), None);
    Ok(())
}

fn assert_foundation_hummus_nutrition(food: &FoodRow) -> Result<(), Box<dyn StdError>> {
    let profile = food
        .parsed_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("foundation hummus sugars were not persisted"))?;
    let expected_total_sugars: Reported<Mass> = "0.34 g".parse()?;

    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(
        profile.carbohydrates.sugars.total.as_ref(),
        Some(&expected_total_sugars)
    );
    assert_eq!(profile.fat.total.as_ref(), None);
    assert_eq!(profile.fat.saturated.as_ref(), None);
    assert_eq!(profile.fat.monounsaturated.as_ref(), None);
    assert_eq!(profile.fat.polyunsaturated.as_ref(), None);
    assert_eq!(profile.fat.trans.as_ref(), None);
    assert_eq!(profile.carbohydrates.including_fibre.as_ref(), None);
    assert_eq!(profile.carbohydrates.excluding_fibre.as_ref(), None);
    assert_eq!(profile.carbohydrates.sugars.added.as_ref(), None);
    assert_eq!(profile.carbohydrates.sugars.lactose.as_ref(), None);
    assert_eq!(profile.carbohydrates.starch.as_ref(), None);
    assert_eq!(profile.fibre.as_ref(), None);
    assert_eq!(profile.protein.as_ref(), None);
    assert_eq!(profile.sodium.as_ref(), None);
    assert_eq!(profile.cholesterol.as_ref(), None);
    assert_eq!(profile.energy.as_ref(), None);
    assert_eq!(profile.salt.as_ref(), None);
    Ok(())
}

fn assert_energy(food: &FoodRow, expected: Option<&str>) -> Result<(), Box<dyn StdError>> {
    let energy = food
        .parsed_data
        .nutrition
        .as_ref()
        .and_then(|profile| profile.energy.as_ref());
    match expected {
        Some(value) => {
            let expected: Reported<Energy> = value.parse()?;
            assert_eq!(energy, Some(&expected), "energy for {}", food.description);
        }
        None => assert_eq!(energy, None, "energy for {}", food.description),
    }
    Ok(())
}

async fn assert_invalid_energy_record_two(
    input: &str,
    filename: &str,
    first_id: &str,
    second_id: &str,
) -> Result<(), Box<dyn StdError>> {
    let (directory, database) = open_database().await?;
    let path = write_input(directory.path(), filename, input)?;
    let error = require_import_error(&database, &path).await?;

    assert!(error.to_string().contains("record 2"), "{error}");
    assert!(error.to_string().contains("committed 1 foods"), "{error}");
    assert_eq!(food_count(&database).await?, 1);
    food_row(&database, "usda", first_id).await?;
    assert_row_absent(&database, "usda", second_id).await?;
    Ok(())
}

type SourceText = String;
type FoodTypeText = String;
type ParsedDataJson = String;
type RowId = i64;
type FoodCount = i64;
type WriteCount = usize;

#[derive(Debug)]
struct FoodRow {
    id: RowId,
    source: SourceText,
    food_type: FoodTypeText,
    description: FoodDescription,
    category: Option<FoodCategory>,
    parsed_data: ParsedFoodData,
}

async fn food_row(
    database: &Database,
    source: &str,
    source_food_id: &str,
) -> Result<FoodRow, Box<dyn StdError>> {
    let (id, source, food_type, description, category, parsed_data): (
        RowId,
        SourceText,
        FoodTypeText,
        FoodDescription,
        Option<FoodCategory>,
        ParsedDataJson,
    ) = sqlx::query_as(
        "SELECT id, source, food_type, description, category, parsed_data
         FROM crumbs_foods
         WHERE source = ? AND source_food_id = ?",
    )
    .bind(source)
    .bind(source_food_id)
    .fetch_one(&database.pool)
    .await?;
    Ok(FoodRow {
        id,
        source,
        food_type,
        description,
        category,
        parsed_data: serde_json::from_str(&parsed_data)?,
    })
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
    parsed_data: ParsedFoodData,
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
