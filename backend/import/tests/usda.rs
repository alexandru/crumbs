use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use sqlx::{FromRow, SqlitePool, query, query_scalar};

use common::DatabaseFixture;
use crumbs_database::nutrition::{FoodData, NutritionBasis, ServingSize};
use crumbs_database::{
    Database, Food, FoodCategory, FoodDescription, FoodId, FoodNotes, FoodWriter, SourceFoodId,
    TagName,
};
use crumbs_import::{
    DiagnosticKind, ImportDiagnostic, ImportError, import_usda, import_usda_with_diagnostics,
};

mod common;

const FOUNDATION_JSON: &str = include_str!("../test-fixtures/foundation.json");
const BRANDED_JSON: &str = include_str!("../test-fixtures/branded.json");
const IMPORT_TIMEOUT: Duration = Duration::from_secs(5);
const LATER_TRIGGER_ROWS: usize = 100;

#[tokio::test]
async fn persists_survey_wweia_category() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let record = r#"{"fdcId":2705383,"description":"Milk, human","dataType":"Survey (FNDDS)","wweiaFoodCategory":{"wweiaFoodCategoryDescription":"Human milk","wweiaFoodCategoryCode":3298314}}"#;
    let input = format!(r#"{{"SurveyFoods":[{record}]}}"#);
    let path = write_input(fixture.directory.path(), "survey.json", &input)?;

    let report = import_usda(&fixture.database, &path).await?;

    assert_eq!(report.committed().get(), 1);
    let food = food_row(&fixture.pool, "usda", "2705383").await?;
    assert_eq!(food.category.as_deref(), Some("Human milk"));
    assert_eq!(food.parsed_data()?, FoodData::default());
    Ok(())
}

#[tokio::test]
async fn imports_foundation_foods_with_typed_data() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let path = write_input(fixture.directory.path(), "foundation.json", FOUNDATION_JSON)?;

    let report = import_usda(&fixture.database, &path).await?;

    assert_eq!(report.committed().get(), 2);
    assert_eq!(food_count(&fixture.pool).await?, 2);
    let food = food_row(&fixture.pool, "usda", "321358").await?;
    assert!(food.id > 0);
    assert_eq!(food.source, "usda");
    assert_eq!(food.dataset, "common");
    assert_eq!(food.description, "Hummus, commercial");
    assert_eq!(food.notes, None);
    assert_eq!(
        food.category.as_deref(),
        Some("Legumes and Legume Products")
    );
    assert!(fixture.database.list_tags().await?.is_empty());
    let parsed_data = food.parsed_data()?;
    assert_eq!(parsed_data.nutrition, None);
    assert_eq!(parsed_data.serving_sizes.len(), 1);
    assert_eq!(
        serving_size_at(&parsed_data, 0)?.quantity().to_string(),
        "33.9 g"
    );
    let food = food_row(&fixture.pool, "usda", "321360").await?;
    let parsed_data = food.parsed_data()?;
    assert_eq!(parsed_data.serving_sizes.len(), 1);
    assert_eq!(
        serving_size_at(&parsed_data, 0)?.quantity().to_string(),
        "49.7 g"
    );
    Ok(())
}

#[tokio::test]
async fn imports_branded_food_with_typed_data() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let path = write_input(fixture.directory.path(), "branded.json", BRANDED_JSON)?;

    let report = import_usda(&fixture.database, &path).await?;

    assert_eq!(report.committed().get(), 1);
    let food = food_row(&fixture.pool, "usda", "1106281").await?;
    assert_eq!(food.source, "usda");
    assert_eq!(food.dataset, "branded");
    assert_eq!(food.description, "GRANOLA, CINNAMON, RAISIN");
    assert_eq!(food.notes, None);
    assert_eq!(food.category.as_deref(), Some("Cereal"));
    assert!(fixture.database.list_tags().await?.is_empty());
    let parsed_data = food.parsed_data()?;
    assert_eq!(parsed_data.serving_sizes.len(), 1);
    assert_eq!(
        serving_size_at(&parsed_data, 0)?.quantity().to_string(),
        "28 g"
    );
    Ok(())
}

#[tokio::test]
async fn imports_typed_nutrition_for_all_supported_usda_food_kinds() -> Result<(), Box<dyn StdError>>
{
    let fixture = DatabaseFixture::open().await?;
    let high_precision_protein = "7.1234567890123456789012345678";
    let foundation_energy = vec![
        nutrient(2048, "2048", "Atwater Specific Factors", "kcal", "91"),
        nutrient(2047, "2047", "Atwater General Factors", "kcal", "92"),
        nutrient(1008, "208", "Energy", "kcal", "93"),
        nutrient(1062, "268", "Energy", "kJ", "389"),
    ];
    let legacy_energy = vec![
        nutrient(2047, "2047", "Atwater General Factors", "kcal", "81"),
        nutrient(1008, "208", "Energy", "kcal", "82"),
        nutrient(1062, "268", "Energy", "kJ", "342"),
    ];
    let survey_energy = vec![
        nutrient(1008, "208", "Energy", "kcal", "71"),
        nutrient(1062, "268", "Energy", "kJ", "300"),
    ];
    let branded_energy = vec![nutrient(1062, "268", "Energy", "kJ", "44")];
    let branded_volume_energy = vec![nutrient(1062, "268", "Energy", "kJ", "55.4")];
    let foundation_portions = format!(
        r#","foodPortions":[{},{}]"#,
        food_portion(1, "cup", "100.25"),
        food_portion(2, "tablespoon", "33.9"),
    );
    let cases = [
        NutritionImportCase {
            root: "FoundationFoods",
            data_type: "Foundation",
            fdc_id: 9401,
            nutrients: test_nutrients(&foundation_energy, high_precision_protein, Some("1.25")),
            extra: foundation_portions,
            expected_basis: NutritionBasis::Per100Grams,
            expected_energy: "91 kcal",
            expected_protein: "7.1234567890123456789012345678 g",
            expected_lactose: Some("1.25 g"),
            expected_servings: vec!["100.25 g", "33.9 g"],
        },
        NutritionImportCase {
            root: "SRLegacyFoods",
            data_type: "SR Legacy",
            fdc_id: 9402,
            nutrients: test_nutrients(&legacy_energy, "0", None),
            extra: String::new(),
            expected_basis: NutritionBasis::Per100Grams,
            expected_energy: "81 kcal",
            expected_protein: "0 g",
            expected_lactose: None,
            expected_servings: Vec::new(),
        },
        NutritionImportCase {
            root: "SurveyFoods",
            data_type: "Survey (FNDDS)",
            fdc_id: 9403,
            nutrients: test_nutrients(&survey_energy, "2.5", None),
            extra: String::new(),
            expected_basis: NutritionBasis::Per100Grams,
            expected_energy: "71 kcal",
            expected_protein: "2.5 g",
            expected_lactose: None,
            expected_servings: Vec::new(),
        },
        NutritionImportCase {
            root: "BrandedFoods",
            data_type: "Branded",
            fdc_id: 9404,
            nutrients: test_nutrients(&branded_energy, "7.5", None),
            extra: r#","servingSize":28,"servingSizeUnit":"g","householdServingFullText":"1 bar","labelNutrients":{"protein":{"value":999}}"#.to_owned(),
            expected_basis: NutritionBasis::Per100Grams,
            expected_energy: "44 kJ",
            expected_protein: "7.5 g",
            expected_lactose: None,
            expected_servings: vec!["28 g"],
        },
        NutritionImportCase {
            root: "BrandedFoods",
            data_type: "Branded",
            fdc_id: 9405,
            nutrients: test_nutrients(&branded_volume_energy, "8", None),
            extra: r#","servingSize":30.5,"servingSizeUnit":"ml","householdServingFullText":"2 tbsp","labelNutrients":{"protein":{"value":888}}"#.to_owned(),
            expected_basis: NutritionBasis::Per100Millilitres,
            expected_energy: "55.4 kJ",
            expected_protein: "8 g",
            expected_lactose: None,
            expected_servings: vec!["30.5 ml"],
        },
    ];

    for case in cases {
        let record = food_record(case.fdc_id, case.data_type, &case.nutrients, &case.extra);
        let input = food_document(case.root, &[record]);
        let path = write_input(
            fixture.directory.path(),
            &format!("{}.json", case.fdc_id),
            &input,
        )?;
        let report = import_usda(&fixture.database, &path).await?;
        assert_eq!(report.committed().get(), 1);

        let row = food_row(&fixture.pool, "usda", &case.fdc_id.to_string()).await?;
        let parsed_data = row.parsed_data()?;
        let profile = parsed_data
            .nutrition
            .as_ref()
            .ok_or_else(|| io::Error::other("import did not persist a nutrition profile"))?;
        assert_eq!(profile.per, case.expected_basis);
        assert_eq!(
            profile.energy.as_ref().map(ToString::to_string).as_deref(),
            Some(case.expected_energy)
        );
        assert_eq!(
            profile.protein.as_ref().map(ToString::to_string).as_deref(),
            Some(case.expected_protein)
        );
        assert_eq!(
            profile.sodium.as_ref().map(ToString::to_string).as_deref(),
            Some("1.234 mg")
        );
        assert_eq!(
            profile
                .carbohydrates
                .including_fibre
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("4.5 g")
        );
        assert_eq!(profile.carbohydrates.excluding_fibre, None);
        assert_eq!(
            profile
                .carbohydrates
                .sugars
                .lactose
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            case.expected_lactose
        );
        assert_eq!(
            parsed_data.serving_sizes.len(),
            case.expected_servings.len()
        );
        for (index, expected) in case.expected_servings.iter().enumerate() {
            assert_eq!(
                serving_size_at(&parsed_data, index)?.quantity().to_string(),
                *expected
            );
        }
    }

    Ok(())
}

#[tokio::test]
async fn unsupported_nutrition_unit_sanitizes_only_its_nutrient() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let first_nutrients = test_nutrients(&[], "3.25", None);
    let first = food_record(9501, "Foundation", &first_nutrients, "");
    let invalid_nutrient = nutrient(1003, "203", "Protein", "oz", "1");
    let second = food_record(
        9502,
        "Foundation",
        &[
            invalid_nutrient,
            nutrient(1004, "204", "Total lipid (fat)", "g", "2"),
        ],
        "",
    );
    let third = food_record(9503, "Foundation", &test_nutrients(&[], "2", None), "");
    let input = food_document("FoundationFoods", &[first, second, third]);
    let path = write_input(fixture.directory.path(), "invalid-nutrition.json", &input)?;

    let mut diagnostics = Vec::new();
    let report = import_usda_with_diagnostics(&fixture.database, &path, |diagnostic| {
        diagnostics.push(diagnostic.clone());
        Ok(())
    })
    .await?;

    assert_eq!(report.committed().get(), 3);
    assert_eq!(report.sanitized().get(), 1);
    assert_eq!(report.rejected().get(), 0);
    assert_eq!(food_count(&fixture.pool).await?, 3);
    let first_food = food_row(&fixture.pool, "usda", "9501").await?;
    assert_eq!(
        first_food
            .parsed_data()?
            .nutrition
            .as_ref()
            .and_then(|profile| profile.protein.as_ref())
            .map(ToString::to_string)
            .as_deref(),
        Some("3.25 g")
    );
    let second_food = food_row(&fixture.pool, "usda", "9502").await?;
    let second_profile = second_food
        .parsed_data()?
        .nutrition
        .ok_or_else(|| io::Error::other("valid sibling nutrient was not persisted"))?;
    assert_eq!(second_profile.protein, None);
    assert_eq!(
        second_profile
            .fat
            .total
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("2 g")
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::Sanitized
            && diagnostic.source_food_id.as_deref() == Some("9502")
            && diagnostic.field.as_deref() == Some("nutrient[1003]")
    }));
    food_row(&fixture.pool, "usda", "9503").await?;
    Ok(())
}

#[tokio::test]
async fn reimport_updates_in_place_and_retains_internal_id() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let path = write_input(fixture.directory.path(), "foundation.json", FOUNDATION_JSON)?;
    import_usda(&fixture.database, &path).await?;
    let id_before = food_row(&fixture.pool, "usda", "321358").await?.id;
    let reviewed_tag = TagName::new("Nutrition reviewed")?;
    fixture
        .database
        .set_food_tags(FoodId(id_before), std::slice::from_ref(&reviewed_tag))
        .await?;

    query(
        "UPDATE crumbs_foods
         SET description = 'stale', category = NULL, parsed_data = '{\"nutrition\":null,\"serving_sizes\":[]}'
         WHERE source = 'usda' AND source_food_id = '321358'",
    )
    .execute(&fixture.pool)
    .await?;

    let report = import_usda(&fixture.database, &path).await?;

    assert_eq!(report.committed().get(), 2);
    assert_eq!(food_count(&fixture.pool).await?, 2);
    let food = food_row(&fixture.pool, "usda", "321358").await?;
    assert_eq!(food.id, id_before, "upsert replaced the internal row id");
    assert_eq!(food.description, "Hummus, commercial");
    assert_eq!(
        food.category.as_deref(),
        Some("Legumes and Legume Products")
    );
    let parsed_data = food.parsed_data()?;
    assert_eq!(parsed_data.serving_sizes.len(), 1);
    assert_eq!(
        serving_size_at(&parsed_data, 0)?.quantity().to_string(),
        "33.9 g"
    );
    let tagged_food_ids: Vec<RowId> = query_scalar(
        "SELECT ft.food_id FROM crumbs_food_tags AS ft
         JOIN crumbs_tags AS t ON t.id = ft.tag_id
         WHERE t.name = ? ORDER BY ft.food_id",
    )
    .bind(reviewed_tag.as_str())
    .fetch_all(&fixture.pool)
    .await?;
    assert_eq!(tagged_food_ids, vec![id_before]);
    Ok(())
}

#[tokio::test]
async fn cross_source_same_food_id_is_isolated() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let path = write_input(fixture.directory.path(), "foundation.json", FOUNDATION_JSON)?;
    import_usda(&fixture.database, &path).await?;
    query("INSERT INTO crumbs_sources (id, created_at) VALUES ('other', 0)")
        .execute(&fixture.pool)
        .await?;
    let other_food_id: RowId = query_scalar(
        "INSERT INTO crumbs_foods (
            source, source_food_id, dataset, description, category, parsed_data, created_at
         ) VALUES ('other', '321358', 'Foundation', 'Other source food', NULL, '{}', 0)
         RETURNING id",
    )
    .fetch_one(&fixture.pool)
    .await?;
    let other_tag = TagName::new("Other")?;
    fixture
        .database
        .set_food_tags(FoodId(other_food_id), std::slice::from_ref(&other_tag))
        .await?;

    import_usda(&fixture.database, &path).await?;

    let other = food_row(&fixture.pool, "other", "321358").await?;
    assert_eq!(other.source, "other");
    assert_eq!(other.description, "Other source food");
    assert_eq!(
        serde_json::from_str::<FoodData>(&other.parsed_data)?,
        FoodData::default()
    );
    let tagged_foods: Vec<RowId> = query_scalar(
        "SELECT ft.food_id FROM crumbs_food_tags ft
         JOIN crumbs_tags t ON t.id = ft.tag_id
         WHERE t.name = ? ORDER BY ft.food_id",
    )
    .bind(other_tag.as_str())
    .fetch_all(&fixture.pool)
    .await?;
    assert_eq!(tagged_foods, vec![other_food_id]);
    let usda = food_row(&fixture.pool, "usda", "321358").await?;
    assert_eq!(usda.description, "Hummus, commercial");
    assert_eq!(food_count(&fixture.pool).await?, 3);
    Ok(())
}

#[tokio::test]
async fn importing_a_subset_retains_absent_rows_and_coexists() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let foundation = write_input(fixture.directory.path(), "foundation.json", FOUNDATION_JSON)?;
    let branded = write_input(fixture.directory.path(), "branded.json", BRANDED_JSON)?;

    import_usda(&fixture.database, &foundation).await?;
    let report = import_usda(&fixture.database, &branded).await?;

    assert_eq!(report.committed().get(), 1);
    assert_eq!(food_count(&fixture.pool).await?, 3);
    food_row(&fixture.pool, "usda", "321358").await?;
    food_row(&fixture.pool, "usda", "321360").await?;
    food_row(&fixture.pool, "usda", "1106281").await?;
    Ok(())
}

#[tokio::test]
async fn imports_supported_roots_into_shared_search_datasets() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let sr = write_input(
        fixture.directory.path(),
        "sr.json",
        r#"{"SRLegacyFoods":[{"fdcId":9301,"description":"Legacy food","dataType":"SR Legacy"}]}"#,
    )?;
    let survey = write_input(
        fixture.directory.path(),
        "survey.json",
        r#"{"SurveyFoods":[{"fdcId":9302,"description":"Survey food","dataType":"Survey (FNDDS)"}]}"#,
    )?;

    import_usda(&fixture.database, &sr).await?;
    import_usda(&fixture.database, &survey).await?;

    let sr_food = food_row(&fixture.pool, "usda", "9301").await?;
    let survey_food = food_row(&fixture.pool, "usda", "9302").await?;
    assert_eq!(sr_food.dataset, "common");
    assert_eq!(survey_food.dataset, "common");
    Ok(())
}

#[tokio::test]
async fn empty_recognized_array_succeeds_with_zero_commits() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let path = write_input(
        fixture.directory.path(),
        "empty.json",
        r#"{"FoundationFoods":[]}"#,
    )?;

    let report = import_usda(&fixture.database, &path).await?;

    assert_eq!(report.committed().get(), 0);
    assert_eq!(food_count(&fixture.pool).await?, 0);
    Ok(())
}

#[tokio::test]
async fn rejects_invalid_document_framing_and_root_inputs() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let cases = [
        ("", "invalid USDA FoodData Central document"),
        ("{}", "empty object"),
        (r#"{"OtherFoods":[]}"#, "unsupported USDA root"),
        (r#"{"FoundationFoods":{}}"#, "invalid type"),
        (r#"{"FoundationFoods":[{"fdcId":1,}]}"#, "record 1"),
    ];

    for (index, (contents, message)) in cases.into_iter().enumerate() {
        let name = format!("invalid-{index}.json");
        let path = write_input(fixture.directory.path(), &name, contents)?;
        let error = require_import_error(&fixture.database, &path).await?;

        assert!(
            error.to_string().contains(message),
            "unexpected error for {contents:?}: {error}"
        );
        assert!(error.to_string().contains("committed 0 foods"));
        assert!(error.to_string().contains(&path.display().to_string()));
        assert_eq!(error.report().committed().get(), 0);
        assert_eq!(error.report().sanitized().get(), 0);
        assert_eq!(error.report().rejected().get(), 0);
        assert_eq!(food_count(&fixture.pool).await?, 0);
    }
    Ok(())
}

#[tokio::test]
async fn import_continues_after_sanitized_and_rejected_foods_with_diagnostics()
-> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let first = food_record(9601, "Foundation", &test_nutrients(&[], "3", None), "");
    let sanitized_nutrients = [
        nutrient(2048, "2048", "Atwater Specific Factors", "kcal", "-1"),
        nutrient(2047, "2047", "Atwater General Factors", "kcal", "42"),
        nutrient(1003, "203", "Protein", "g", "-2"),
        r#"{"nutrient":{"id":1093,"number":"307","name":"Sodium","unitName":"mg"},"amount":1.5,"loq":0}"#.to_owned(),
        r#"{"nutrient":{"id":1079,"number":"291","name":"Fiber","unitName":"g"},"amount":null,"loq":0}"#.to_owned(),
        r#"{"nutrient":{"id":1009,"number":"209","name":"Starch","unitName":"g"},"amount":0.2,"loq":0.55}"#.to_owned(),
        r#"{"nutrient":{"id":1235,"number":"539","name":"Added sugars","unitName":"g"},"amount":1,"loq":-0.1}"#.to_owned(),
    ];
    let sanitized = food_record(9602, "Foundation", &sanitized_nutrients, "");
    let missing_description =
        r#"{"fdcId":9603,"dataType":"Foundation","foodNutrients":[]}"#.to_owned();
    let invalid_unit = food_record(
        9604,
        "Foundation",
        &[
            nutrient(1003, "203", "Protein", "oz", "1"),
            nutrient(1004, "204", "Total lipid (fat)", "g", "2"),
        ],
        "",
    );
    let invalid_precision = food_record(
        9605,
        "Foundation",
        &[nutrient(
            1003,
            "203",
            "Protein",
            "g",
            "0.12345678901234567890123456789",
        )],
        "",
    );
    let impossible_bound = food_record(
        9606,
        "Foundation",
        &[nutrient(1003, "203", "Protein", "g", "\"< 0\"")],
        "",
    );
    let invalid_serving = food_record(
        9607,
        "Foundation",
        &[],
        r#", "foodPortions":[{"portionDescription":"zero portion","gramWeight":0}]"#,
    );
    let last = food_record(9608, "Foundation", &test_nutrients(&[], "8", None), "");
    let input = food_document(
        "FoundationFoods",
        &[
            first,
            sanitized,
            missing_description,
            invalid_unit,
            invalid_precision,
            impossible_bound,
            invalid_serving,
            last,
        ],
    );
    let path = write_input(fixture.directory.path(), "recoverable.json", &input)?;
    let mut diagnostics = Vec::<ImportDiagnostic>::new();

    let report = import_usda_with_diagnostics(&fixture.database, &path, |diagnostic| {
        diagnostics.push(diagnostic.clone());
        Ok(())
    })
    .await?;

    assert_eq!(report.committed().get(), 7);
    assert_eq!(report.sanitized().get(), 5);
    assert_eq!(report.rejected().get(), 1);
    assert_eq!(food_count(&fixture.pool).await?, 7);
    food_row(&fixture.pool, "usda", "9601").await?;
    food_row(&fixture.pool, "usda", "9608").await?;
    let sanitized_row = food_row(&fixture.pool, "usda", "9602").await?;
    let parsed = sanitized_row.parsed_data()?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("sanitized food has no nutrition profile"))?;
    assert_eq!(
        profile.energy.as_ref().map(ToString::to_string).as_deref(),
        Some("42 kcal")
    );
    assert_eq!(profile.protein, None);
    assert_eq!(
        profile.sodium.as_ref().map(ToString::to_string).as_deref(),
        Some("1.5 mg")
    );
    assert_eq!(profile.fibre, None);
    assert_eq!(
        profile
            .carbohydrates
            .starch
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("0.2 g")
    );
    assert_eq!(
        profile
            .carbohydrates
            .sugars
            .added
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("1 g")
    );
    assert!(!serde_json::to_string(&parsed)?.contains("< 0"));
    let invalid_unit_row = food_row(&fixture.pool, "usda", "9604").await?;
    let invalid_unit_profile = invalid_unit_row
        .parsed_data()?
        .nutrition
        .ok_or_else(|| io::Error::other("valid nutrient sibling was not persisted"))?;
    assert_eq!(invalid_unit_profile.protein, None);
    assert_eq!(
        invalid_unit_profile
            .fat
            .total
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("2 g")
    );

    let sanitized_diagnostics: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::Sanitized)
        .collect();
    assert_eq!(sanitized_diagnostics.len(), 9);
    for (source_food_id, field) in [
        ("9602", "energy"),
        ("9602", "protein"),
        ("9602", "sodium"),
        ("9602", "fibre"),
        ("9602", "carbohydrates.sugars.added"),
        ("9604", "nutrient[1003]"),
        ("9605", "nutrient[1003]"),
        ("9606", "nutrient[1003]"),
    ] {
        assert!(
            sanitized_diagnostics.iter().any(|diagnostic| {
                diagnostic.source == "usda"
                    && diagnostic.source_food_id.as_deref() == Some(source_food_id)
                    && diagnostic.field.as_deref() == Some(field)
                    && !diagnostic.reason.is_empty()
            }),
            "missing sanitization diagnostic for {source_food_id} {field}"
        );
    }
    assert!(sanitized_diagnostics.iter().any(|diagnostic| {
        diagnostic.source_food_id.as_deref() == Some("9607")
            && diagnostic
                .field
                .as_deref()
                .is_some_and(|field| field.starts_with("serving_sizes"))
    }));

    let rejected_diagnostics: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::Rejected)
        .collect();
    assert_eq!(rejected_diagnostics.len(), 1);
    for source_food_id in ["9603"] {
        assert!(
            rejected_diagnostics.iter().any(|diagnostic| {
                diagnostic.source == "usda"
                    && diagnostic.source_food_id.as_deref() == Some(source_food_id)
                    && diagnostic.position.get() > 0
                    && !diagnostic.reason.is_empty()
            }),
            "missing rejected diagnostic for {source_food_id}"
        );
        assert_row_absent(&fixture.pool, "usda", source_food_id).await?;
    }
    Ok(())
}

#[tokio::test]
async fn sanitizes_optional_fields_independently_and_counts_each_food_once()
-> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let mixed = r#"{"fdcId":9701,"description":"Mixed optional data","dataType":"Foundation","foodCategory":17,"foodNutrients":[
      {"nutrient":{"id":1003,"unitName":false},"amount":1},
      {"nutrient":{"id":1004,"unitName":"g"},"amount":2},
      {"nutrient":{"id":1079,"unitName":"g"},"amount":-1},
      {"nutrient":{"id":1093,"unitName":"mg"},"amount":1,"loq":"~ 0.5"}
    ],"foodPortions":[{"portionDescription":"bad portion","gramWeight":{}},{"portionDescription":"cup","gramWeight":30}]}"#;
    let later = food_record(9702, "Foundation", &test_nutrients(&[], "3", None), "");
    let invalid_identity =
        r#"{"fdcId":0,"description":"Invalid identity","dataType":"Foundation"}"#.to_owned();
    let input = food_document(
        "FoundationFoods",
        &[mixed.to_owned(), later, invalid_identity],
    );
    let path = write_input(fixture.directory.path(), "optional-fields.json", &input)?;
    let mut diagnostics = Vec::<ImportDiagnostic>::new();

    let report = import_usda_with_diagnostics(&fixture.database, &path, |diagnostic| {
        diagnostics.push(diagnostic.clone());
        Ok(())
    })
    .await?;

    assert_eq!(report.committed().get(), 2);
    assert_eq!(report.sanitized().get(), 1);
    assert_eq!(report.rejected().get(), 1);
    assert_eq!(food_count(&fixture.pool).await?, 2);

    let food = food_row(&fixture.pool, "usda", "9701").await?;
    assert_eq!(food.category, None);
    let parsed = food.parsed_data()?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("valid nutrient was not persisted"))?;
    assert_eq!(profile.protein, None);
    assert_eq!(
        profile
            .fat
            .total
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("2 g")
    );
    assert_eq!(profile.fibre, None);
    assert_eq!(profile.sodium, None);
    assert_eq!(parsed.serving_sizes.len(), 1);
    assert_eq!(serving_size_at(&parsed, 0)?.quantity().to_string(), "30 g");
    food_row(&fixture.pool, "usda", "9702").await?;
    assert_row_absent(&fixture.pool, "usda", "0").await?;

    let sanitized: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::Sanitized)
        .collect();
    assert_eq!(sanitized.len(), 5);
    assert!(sanitized.iter().all(|diagnostic| {
        diagnostic.source_food_id.as_deref() == Some("9701")
            && diagnostic.field.is_some()
            && !diagnostic.reason.is_empty()
    }));
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == DiagnosticKind::Rejected)
            .count(),
        1
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::Rejected
            && diagnostic.source_food_id.as_deref() == Some("0")
    }));
    Ok(())
}

#[tokio::test]
async fn diagnostic_callback_failure_returns_completed_counts_without_deadlock()
-> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let sanitized = food_record(
        9611,
        "Foundation",
        &[nutrient(1003, "203", "Protein", "g", "-1")],
        "",
    );
    let later = food_record(9612, "Foundation", &[], "");
    let input = food_document("FoundationFoods", &[sanitized, later]);
    let path = write_input(fixture.directory.path(), "diagnostic-failure.json", &input)?;

    let result = tokio::time::timeout(
        IMPORT_TIMEOUT,
        import_usda_with_diagnostics(&fixture.database, &path, |_| {
            Err(io::Error::other("diagnostic output failed"))
        }),
    )
    .await
    .map_err(|_| io::Error::other("import deadlocked after diagnostic callback failed"))?;
    let error = result
        .err()
        .ok_or_else(|| io::Error::other("import unexpectedly succeeded"))?;

    assert!(
        error.to_string().contains("diagnostic output failed"),
        "{error}"
    );
    assert_eq!(error.report().committed().get(), 1);
    assert_eq!(error.report().sanitized().get(), 1);
    assert_eq!(error.report().rejected().get(), 0);
    assert_eq!(food_count(&fixture.pool).await?, 1);
    food_row(&fixture.pool, "usda", "9611").await?;
    assert_row_absent(&fixture.pool, "usda", "9612").await?;
    Ok(())
}

#[tokio::test]
async fn late_framing_error_keeps_committed_food() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let path = write_input(
        fixture.directory.path(),
        "late.json",
        r#"{"FoundationFoods":[{"fdcId":9001,"description":"Late food","dataType":"Foundation"}]} trailing"#,
    )?;

    let error = require_import_error(&fixture.database, &path).await?;

    assert!(error.to_string().contains("committed 1 foods"), "{error}");
    assert_eq!(error.report().committed().get(), 1);
    assert_eq!(error.report().sanitized().get(), 0);
    assert_eq!(error.report().rejected().get(), 0);
    assert!(error.source().is_some(), "import error lost its source");
    assert!(
        error.source().and_then(StdError::source).is_some(),
        "stream error lost its nested JSON source"
    );
    assert_eq!(food_count(&fixture.pool).await?, 1);
    food_row(&fixture.pool, "usda", "9001").await?;
    Ok(())
}

#[tokio::test]
async fn malformed_required_header_rejects_food_and_continues() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let path = write_input(
        fixture.directory.path(),
        "partial.json",
        r#"{"FoundationFoods":[
            {"fdcId":9101,"description":"First food","dataType":"Foundation"},
            {"fdcId":9102,"dataType":"Foundation"},
            {"fdcId":9103,"description":"Third food","dataType":"Foundation"}
        ]}"#,
    )?;

    let report = import_usda(&fixture.database, &path).await?;

    assert_eq!(report.committed().get(), 2);
    assert_eq!(report.sanitized().get(), 0);
    assert_eq!(report.rejected().get(), 1);
    assert_eq!(food_count(&fixture.pool).await?, 2);
    food_row(&fixture.pool, "usda", "9101").await?;
    food_row(&fixture.pool, "usda", "9103").await?;
    assert_row_absent(&fixture.pool, "usda", "9102").await?;
    Ok(())
}

#[tokio::test]
async fn write_failure_rolls_back_only_failed_food_and_retry_succeeds()
-> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let input = trigger_failure_input();
    let path = write_input(fixture.directory.path(), "trigger.json", &input)?;
    query(
        "CREATE TRIGGER fail_trigger_food
         BEFORE INSERT ON crumbs_foods
         WHEN NEW.description = 'Trigger fail food'
         BEGIN
             SELECT RAISE(ABORT, 'induced write failure');
         END",
    )
    .execute(&fixture.pool)
    .await?;

    let error = require_import_error(&fixture.database, &path).await?;

    assert!(error.to_string().contains("9202"), "{error}");
    assert!(error.to_string().contains("record 2"), "{error}");
    assert!(error.to_string().contains("committed 1 foods"), "{error}");
    assert_eq!(error.report().committed().get(), 1);
    assert_eq!(error.report().sanitized().get(), 0);
    assert_eq!(error.report().rejected().get(), 0);
    assert!(
        error.to_string().contains("induced write failure"),
        "{error}"
    );
    assert!(error.source().is_some(), "import error lost its source");
    assert_eq!(food_count(&fixture.pool).await?, 1);
    food_row(&fixture.pool, "usda", "9201").await?;
    assert_row_absent(&fixture.pool, "usda", "9202").await?;
    assert_row_absent(&fixture.pool, "usda", "9203").await?;
    assert_row_absent(&fixture.pool, "usda", "9302").await?;

    query("DROP TRIGGER fail_trigger_food")
        .execute(&fixture.pool)
        .await?;
    let report = import_usda(&fixture.database, &path).await?;

    let expected = 2 + LATER_TRIGGER_ROWS;
    assert_eq!(report.committed().get(), expected);
    assert_eq!(food_count(&fixture.pool).await?, expected as i64);
    food_row(&fixture.pool, "usda", "9202").await?;
    food_row(&fixture.pool, "usda", "9203").await?;
    food_row(&fixture.pool, "usda", "9302").await?;
    Ok(())
}

#[tokio::test]
async fn closed_pool_import_fails_with_bounded_termination() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let path = write_input(fixture.directory.path(), "closed.json", FOUNDATION_JSON)?;
    fixture.close().await;

    let error = require_import_error(&fixture.database, &path).await?;

    let message = error.to_string();
    assert!(
        message.contains("failed to persist USDA food 321358 at record 1"),
        "{message}"
    );
    assert!(message.contains("committed 0 foods"), "{message}");
    assert_eq!(error.report().committed().get(), 0);
    assert_eq!(error.report().sanitized().get(), 0);
    assert_eq!(error.report().rejected().get(), 0);
    assert!(message.contains(&path.display().to_string()), "{message}");
    Ok(())
}

#[tokio::test]
async fn missing_input_reports_path_and_zero_committed() -> Result<(), Box<dyn StdError>> {
    let fixture = DatabaseFixture::open().await?;
    let path = fixture.directory.path().join("missing.json");

    let error = require_import_error(&fixture.database, &path).await?;

    assert!(error.to_string().contains(&path.display().to_string()));
    assert!(error.to_string().contains("committed 0 foods"));
    assert_eq!(error.report().committed().get(), 0);
    assert_eq!(error.report().sanitized().get(), 0);
    assert_eq!(error.report().rejected().get(), 0);
    assert_eq!(food_count(&fixture.pool).await?, 0);
    Ok(())
}

#[tokio::test]
async fn imports_into_generic_food_writer_and_passes_typed_data() -> Result<(), Box<dyn StdError>> {
    let directory = tempfile::tempdir()?;
    let first_record = r#"{"fdcId":7301, "description":"Alternative food", "dataType":"Foundation", "foodCategory":{"description":"Shelf Stable"}, "details":{"measurement":1.00}}"#;
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
    assert_eq!(first_food.dataset, "common");
    assert_eq!(first_food.notes, None);
    assert_eq!(first_food.description, "Alternative food");
    assert_eq!(first_food.category.as_deref(), Some("Shelf Stable"));
    assert_eq!(first_food.parsed_data, FoodData::default());

    let second_record = r#"{"fdcId":7301, "description":"Updated alternative food", "dataType":"Foundation", "foodCategory":{"description":"Shelf Stable"}, "details":{"measurement":2.00}}"#;
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
    assert_eq!(updated_food.parsed_data, FoodData::default());
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

async fn food_count(pool: &SqlitePool) -> Result<FoodCount, sqlx::Error> {
    query_scalar("SELECT COUNT(*) FROM crumbs_foods")
        .fetch_one(pool)
        .await
}

type SourceText = String;
type DatasetText = String;
type ParsedDataText = String;
type RawFdcId = u64;
type RawNutrientId = u16;
type RowId = i64;
type FoodCount = i64;
type WriteCount = usize;

#[derive(Debug, FromRow)]
struct FoodRow {
    id: RowId,
    source: SourceText,
    dataset: DatasetText,
    description: FoodDescription,
    category: Option<FoodCategory>,
    notes: Option<FoodNotes>,
    parsed_data: ParsedDataText,
}

impl FoodRow {
    fn parsed_data(&self) -> Result<FoodData, serde_json::Error> {
        serde_json::from_str(&self.parsed_data)
    }
}

async fn food_row(
    pool: &SqlitePool,
    source: &str,
    source_food_id: &str,
) -> Result<FoodRow, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, source, dataset, description, category, notes, parsed_data
         FROM crumbs_foods
         WHERE source = ? AND source_food_id = ?",
    )
    .bind(source)
    .bind(source_food_id)
    .fetch_one(pool)
    .await
}

struct NutritionImportCase {
    root: &'static str,
    data_type: &'static str,
    fdc_id: RawFdcId,
    nutrients: Vec<String>,
    extra: String,
    expected_basis: NutritionBasis,
    expected_energy: &'static str,
    expected_protein: &'static str,
    expected_lactose: Option<&'static str>,
    expected_servings: Vec<&'static str>,
}

fn nutrient(id: RawNutrientId, number: &str, name: &str, unit: &str, amount: &str) -> String {
    format!(
        r#"{{"nutrient":{{"id":{id},"number":"{number}","name":"{name}","unitName":"{unit}"}},"amount":{amount}}}"#
    )
}

fn test_nutrients(
    energy: &[String],
    protein_amount: &str,
    lactose_amount: Option<&str>,
) -> Vec<String> {
    let mut nutrients = energy.to_vec();
    nutrients.push(nutrient(1003, "203", "Protein", "g", protein_amount));
    nutrients.push(nutrient(
        1005,
        "205",
        "Carbohydrate, by difference",
        "g",
        "4.5",
    ));
    nutrients.push(nutrient(1093, "307", "Sodium, Na", "mg", "1.234"));
    if let Some(amount) = lactose_amount {
        nutrients.push(nutrient(1013, "213", "Lactose", "g", amount));
    }
    nutrients
}

fn food_portion(sequence: usize, name: &str, gram_weight: &str) -> String {
    format!(
        r#"{{"id":{sequence},"value":1.0,"measureUnit":{{"id":1001,"name":"{name}","abbreviation":"{name}"}},"modifier":"","gramWeight":{gram_weight},"sequenceNumber":{sequence},"amount":1.0}}"#
    )
}

fn food_record(fdc_id: RawFdcId, data_type: &str, nutrients: &[String], extra: &str) -> String {
    format!(
        r#"{{"fdcId":{fdc_id},"description":"Nutrition test food {fdc_id}","dataType":"{data_type}","foodNutrients":[{}]{extra}}}"#,
        nutrients.join(",")
    )
}

fn food_document(root: &str, foods: &[String]) -> String {
    format!(r#"{{"{root}":[{}]}}"#, foods.join(","))
}

fn serving_size_at(food_data: &FoodData, index: usize) -> Result<&ServingSize, io::Error> {
    food_data
        .serving_sizes
        .get(index)
        .ok_or_else(|| io::Error::other(format!("missing serving size at position {index}")))
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
    dataset: DatasetText,
    description: FoodDescription,
    category: Option<FoodCategory>,
    notes: Option<String>,
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
            dataset: food.dataset.to_string(),
            description: food.description.clone(),
            category: food.category.clone(),
            notes: food.notes.clone(),
            parsed_data: food.parsed_data.clone(),
        };

        state.foods.insert(key, recorded_food);
        state.successful_writes = successful_writes;
        Ok(id)
    }
}

async fn assert_row_absent(
    pool: &SqlitePool,
    source: &str,
    source_food_id: &str,
) -> Result<(), Box<dyn StdError>> {
    let count: i64 = query_scalar(
        "SELECT COUNT(*) FROM crumbs_foods
         WHERE source = ? AND source_food_id = ?",
    )
    .bind(source)
    .bind(source_food_id)
    .fetch_one(pool)
    .await?;
    assert_eq!(count, 0, "unexpected food row {source}/{source_food_id}");
    Ok(())
}
