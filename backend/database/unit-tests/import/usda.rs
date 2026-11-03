use std::error::Error as StdError;
use std::io;
use std::io::Read;
use std::thread;

use tokio::sync::mpsc;

use crate::measure::{Energy, Mass, MeasuredQuantity};
use crate::parsed::{NutritionBasis, NutritionProfile, ParsedFoodData, ServingName, ServingSize};

use super::*;

const FOUNDATION_RECORD: &str = r#"{"fdcId": 321358, "description": "Hummus, commercial", "dataType": "Foundation", "foodCategory": {"description": "Legumes and Legume Products"}, "publicationDate": "4/1/2019", "foodNutrients": [{"nutrient": {"id": 1120, "number": "334", "unitName": "µg"}, "amount": 1.00, "ratio": 1e+3}], "scientificName": "Solanum lycopersicum", "unknown": {"nested": [true, null, "µg"]}}"#;
const FOUNDATION_RECORD_NULL_CATEGORY: &str = r#"{"fdcId":321360,"description":"Tomatoes, grape, raw","dataType":"Foundation","foodCategory":null}"#;
const BRANDED_RECORD: &str = r#"{"foodClass":"Branded","description":"GRANOLA, CINNAMON, RAISIN","foodNutrients":[],"publicationDate":"11/13/2020","brandOwner":"MICHELE'S","gtinUpc":"001633636543505","dataSource":"LI","servingSize":28,"servingSizeUnit":"g","householdServingFullText":"0.25 cup","brandedFoodCategory":"Cereal","modifiedDate":"4/26/2020","availableDate":"4/26/2020","dataType":"Branded","fdcId":1106281,"labelNutrients":{"calories":{"value":120}}}"#;
const SURVEY_RECORD: &str = r#"{"fdcId":2705383,"description":"Milk, human","dataType":"Survey (FNDDS)","wweiaFoodCategory":{"wweiaFoodCategoryDescription":"Human milk","wweiaFoodCategoryCode":3298314}}"#;
const IO_FAILURE_FIRST_RECORD: &str =
    r#"{"fdcId":1,"description":"First record","dataType":"Foundation"}"#;
const IO_FAILURE_SECOND_RECORD: &str =
    r#"{"fdcId":2,"description":"Second record","dataType":"Foundation"}"#;

#[test]
fn preserves_survey_wweia_category() -> Result<(), Box<dyn StdError>> {
    let input = document("SurveyFoods", &[SURVEY_RECORD]);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;

    let food = first_food(&foods)?;
    assert_eq!(
        food.category.as_ref().map(FoodCategory::as_str),
        Some("Human milk")
    );
    // The record carries no nutrients or portions, so nothing is parsed from it.
    assert_eq!(food.parsed, ParsedFoodData::default());
    Ok(())
}

#[test]
fn survey_category_fallback_preserves_existing_precedence() -> Result<(), Box<dyn StdError>> {
    let records = [
        (
            r#"{"fdcId":9,"description":"Food","dataType":"Survey (FNDDS)","wweiaFoodCategory":["Human milk"]}"#,
            None,
        ),
        (
            r#"{"fdcId":1,"description":"Food","dataType":"Survey (FNDDS)"}"#,
            None,
        ),
        (
            r#"{"fdcId":2,"description":"Food","dataType":"Survey (FNDDS)","foodCategory":null,"wweiaFoodCategory":null}"#,
            None,
        ),
        (
            r#"{"fdcId":3,"description":"Food","dataType":"Survey (FNDDS)","foodCategory":{"description":null},"wweiaFoodCategory":{"wweiaFoodCategoryDescription":"WWEIA"}}"#,
            Some("WWEIA"),
        ),
        (
            r#"{"fdcId":4,"description":"Food","dataType":"Survey (FNDDS)","foodCategory":{"description":"First","description":"Legacy"},"wweiaFoodCategory":{"wweiaFoodCategoryDescription":"WWEIA"}}"#,
            Some("Legacy"),
        ),
        (
            r#"{"fdcId":5,"description":"Food","dataType":"Survey (FNDDS)","wweiaFoodCategory":"invalid"}"#,
            None,
        ),
        (
            r#"{"fdcId":6,"description":"Food","dataType":"Survey (FNDDS)","wweiaFoodCategory":{"wweiaFoodCategoryDescription":42}}"#,
            None,
        ),
        (
            r#"{"fdcId":7,"description":"Food","dataType":"Survey (FNDDS)","wweiaFoodCategory":null,"wweiaFoodCategory":{"wweiaFoodCategoryDescription":"WWEIA"}}"#,
            None,
        ),
        (
            r#"{"fdcId":8,"description":"Food","dataType":"Survey (FNDDS)","wweiaFoodCategory":{"wweiaFoodCategoryDescription":"First","wweiaFoodCategoryDescription":"Second"}}"#,
            Some("Second"),
        ),
    ];

    for (record, expected) in records {
        let input = document("SurveyFoods", &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        result?;
        assert_eq!(
            first_food(&foods)?
                .category
                .as_ref()
                .map(FoodCategory::as_str),
            expected
        );
    }
    Ok(())
}

#[test]
fn ignores_wweia_metadata_for_other_food_kinds() -> Result<(), Box<dyn StdError>> {
    let cases = [
        (
            "FoundationFoods",
            r#"{"fdcId":1,"description":"Food","dataType":"Foundation","wweiaFoodCategory":42}"#,
        ),
        (
            "SRLegacyFoods",
            r#"{"fdcId":2,"description":"Food","dataType":"SR Legacy","wweiaFoodCategory":{"wweiaFoodCategoryDescription":"WWEIA"}}"#,
        ),
        (
            "BrandedFoods",
            r#"{"fdcId":3,"description":"Food","dataType":"Branded","wweiaFoodCategory":null,"wweiaFoodCategory":"invalid"}"#,
        ),
    ];

    for (root, record) in cases {
        let input = document(root, &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        result?;
        assert_eq!(first_food(&foods)?.category, None);
    }
    Ok(())
}

#[test]
fn streams_foundation_foods_and_parses_their_data() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[FOUNDATION_RECORD, FOUNDATION_RECORD_NULL_CATEGORY],
    );

    let (result, foods) = collect(input.as_bytes(), 1);
    result?;

    assert_eq!(foods.len(), 2);
    let first = first_food(&foods)?;
    assert_eq!(first.fdc_id.as_str(), "321358");
    assert_eq!(first.kind, FoodKind::Foundation);
    assert_eq!(first.description.as_str(), "Hummus, commercial");
    assert_eq!(
        first.category.as_ref().map(FoodCategory::as_str),
        Some("Legumes and Legume Products")
    );
    assert_eq!(first.position.get(), 1);
    // Cryptoxanthin is outside the covered nutrients, so the record carries
    // no profile and no servings.
    assert_eq!(first.parsed, ParsedFoodData::default());

    let second = foods
        .get(1)
        .ok_or_else(|| io::Error::other("missing second food"))?;
    assert_eq!(second.fdc_id.as_str(), "321360");
    assert_eq!(second.kind, FoodKind::Foundation);
    assert_eq!(second.category, None);
    assert_eq!(second.position.get(), 2);
    assert_eq!(second.parsed, ParsedFoodData::default());
    Ok(())
}

#[test]
fn streams_branded_food_and_normalizes_label_nutrients() -> Result<(), Box<dyn StdError>> {
    let input = document("BrandedFoods", &[BRANDED_RECORD]);

    let (result, foods) = collect(input.as_bytes(), 1);
    result?;

    assert_eq!(foods.len(), 1);
    let food = first_food(&foods)?;
    assert_eq!(food.fdc_id.as_str(), "1106281");
    assert_eq!(food.kind, FoodKind::Branded);
    assert_eq!(food.description.as_str(), "GRANOLA, CINNAMON, RAISIN");
    assert_eq!(
        food.category.as_ref().map(FoodCategory::as_str),
        Some("Cereal")
    );

    // The label reports 120 kcal per 28 g serving, which normalizes to the
    // 100 g basis with exact decimal arithmetic.
    let serving = food
        .parsed
        .serving_sizes
        .first()
        .ok_or_else(|| io::Error::other("branded serving is missing"))?;
    assert_eq!(serving.name.as_str(), "0.25 cup");
    assert_eq!(serving.quantity, MeasuredQuantity::parse("28 g")?);
    let profile = match food.parsed.nutrition.as_ref() {
        Some(profile) => profile,
        None => return Err(io::Error::other("branded nutrition is missing").into()),
    };
    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(
        profile.energy,
        Some(Energy::parse("428.57142857142857142857142857 kcal")?)
    );
    Ok(())
}

#[test]
fn accepts_sr_legacy_and_survey_roots() -> Result<(), Box<dyn StdError>> {
    let sr_input = document(
        "SRLegacyFoods",
        &[r#"{"fdcId":1,"description":"Legacy food","dataType":"SR Legacy"}"#],
    );
    let (sr_result, sr_foods) = collect(sr_input.as_bytes(), 1);
    sr_result?;
    let sr_food = first_food(&sr_foods)?;
    assert_eq!(sr_food.kind, FoodKind::SrLegacy);

    let survey_input = document(
        "SurveyFoods",
        &[r#"{"fdcId":2,"description":"Survey food","dataType":"Survey (FNDDS)"}"#],
    );
    let (survey_result, survey_foods) = collect(survey_input.as_bytes(), 1);
    survey_result?;
    let survey_food = first_food(&survey_foods)?;
    assert_eq!(survey_food.kind, FoodKind::Survey);
    Ok(())
}

#[test]
fn selects_energy_by_the_project_priority_order() -> Result<(), Box<dyn StdError>> {
    // The order is 2048, 2047, 1008, then 1062, whatever else the record
    // carries; the winner keeps its source unit.
    let cases: [(Vec<String>, &str); 4] = [
        (
            vec![
                nutrient(2048, "kcal", "55.6"),
                nutrient(2047, "kcal", "61.8"),
                nutrient(1008, "kcal", "52.0"),
                nutrient(1062, "kJ", "234"),
            ],
            "55.6 kcal",
        ),
        (
            vec![
                nutrient(2047, "kcal", "61.8"),
                nutrient(1008, "kcal", "52.0"),
                nutrient(1062, "kJ", "234"),
            ],
            "61.8 kcal",
        ),
        (
            vec![nutrient(1008, "kcal", "229"), nutrient(1062, "kJ", "960")],
            "229 kcal",
        ),
        (vec![nutrient(1062, "kJ", "960")], "960 kJ"),
    ];

    for (nutrients, expected) in cases {
        let input = foundation_record(&nutrients);
        let (result, foods) = collect(input.as_bytes(), 1);
        result?;
        let profile = nutrition_of(first_food(&foods)?)?;
        assert_eq!(
            profile.energy,
            Some(Energy::parse(expected)?),
            "wrong energy selection for {expected}"
        );
    }
    Ok(())
}

#[test]
fn falls_through_energy_candidates_without_amounts() -> Result<(), Box<dyn StdError>> {
    let input = foundation_record(&[
        nutrient_without_amount(2048, "kcal"),
        nutrient(2047, "kcal", "61.8"),
    ]);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    let profile = nutrition_of(first_food(&foods)?)?;
    assert_eq!(profile.energy, Some(Energy::parse("61.8 kcal")?));
    Ok(())
}

#[test]
fn leaves_energy_absent_when_no_candidate_is_supplied() -> Result<(), Box<dyn StdError>> {
    let input = foundation_record(&[nutrient(1003, "g", "7.0")]);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    let profile = nutrition_of(first_food(&foods)?)?;
    assert_eq!(profile.energy, None);
    Ok(())
}

#[test]
fn rejects_invalid_energy_candidates_instead_of_masking() -> Result<(), Box<dyn StdError>> {
    // Energy (1008) reported in grams is invalid data; the record fails
    // rather than falling through to the kJ candidate.
    let input = foundation_record(&[nutrient(1008, "g", "52.0"), nutrient(1062, "kJ", "234")]);
    match single_record_error(input)? {
        RecordError::NutrientUnit { nutrient_id, unit } => {
            assert_eq!(nutrient_id, 1008);
            assert_eq!(unit, "g");
        }
        other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
    Ok(())
}

#[test]
fn parses_all_covered_nutrients_in_their_source_units() -> Result<(), Box<dyn StdError>> {
    let input = foundation_record(&[
        nutrient(2048, "kcal", "210"),
        nutrient(1004, "g", "8.4"),
        nutrient(1258, "g", "1.2"),
        nutrient(1292, "g", "3.3"),
        nutrient(1293, "g", "2.9"),
        nutrient(1257, "g", "0.1"),
        nutrient(1005, "g", "28.5"),
        nutrient(1063, "g", "4.2"),
        nutrient(1013, "g", "0.55"),
        nutrient(1009, "g", "20.0"),
        nutrient(1079, "g", "4.0"),
        nutrient(1003, "g", "7.0"),
        nutrient(1093, "mg", "150"),
        nutrient(1253, "mg", "15"),
    ]);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    let profile = nutrition_of(first_food(&foods)?)?;

    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(profile.energy, Some(Energy::parse("210 kcal")?));
    assert_eq!(profile.fat.total, Some(Mass::parse("8.4 g")?));
    assert_eq!(profile.fat.saturated, Some(Mass::parse("1.2 g")?));
    assert_eq!(profile.fat.monounsaturated, Some(Mass::parse("3.3 g")?));
    assert_eq!(profile.fat.polyunsaturated, Some(Mass::parse("2.9 g")?));
    assert_eq!(profile.fat.trans, Some(Mass::parse("0.1 g")?));
    // Carbohydrate by difference includes fibre; the other meaning stays absent.
    assert_eq!(
        profile.carbohydrates.including_fibre,
        Some(Mass::parse("28.5 g")?)
    );
    assert_eq!(profile.carbohydrates.excluding_fibre, None);
    assert_eq!(
        profile.carbohydrates.sugars.total,
        Some(Mass::parse("4.2 g")?)
    );
    assert_eq!(profile.carbohydrates.sugars.added, None);
    assert_eq!(
        profile.carbohydrates.sugars.lactose,
        Some(Mass::parse("0.55 g")?)
    );
    assert_eq!(profile.carbohydrates.starch, Some(Mass::parse("20.0 g")?));
    assert_eq!(profile.fibre, Some(Mass::parse("4.0 g")?));
    assert_eq!(profile.protein, Some(Mass::parse("7.0 g")?));
    assert_eq!(profile.sodium, Some(Mass::parse("150 mg")?));
    // USDA reports sodium, never salt.
    assert_eq!(profile.salt, None);
    assert_eq!(profile.cholesterol, Some(Mass::parse("15 mg")?));
    Ok(())
}

#[test]
fn prefers_the_foundation_sugars_measure() -> Result<(), Box<dyn StdError>> {
    let both = foundation_record(&[nutrient(1063, "g", "4.2"), nutrient(2000, "g", "5.0")]);
    let (result, foods) = collect(both.as_bytes(), 1);
    result?;
    assert_eq!(
        nutrition_of(first_food(&foods)?)?
            .carbohydrates
            .sugars
            .total,
        Some(Mass::parse("4.2 g")?)
    );

    let only_sr = foundation_record(&[nutrient(2000, "g", "5.0")]);
    let (result, foods) = collect(only_sr.as_bytes(), 1);
    result?;
    assert_eq!(
        nutrition_of(first_food(&foods)?)?
            .carbohydrates
            .sugars
            .total,
        Some(Mass::parse("5.0 g")?)
    );
    Ok(())
}

#[test]
fn keeps_reported_zero_distinct_from_missing_amounts() -> Result<(), Box<dyn StdError>> {
    let reported_zero = foundation_record(&[nutrient(1003, "g", "0.0")]);
    let (result, foods) = collect(reported_zero.as_bytes(), 1);
    result?;
    let profile = nutrition_of(first_food(&foods)?)?;
    assert_eq!(profile.protein, Some(Mass::parse("0.0 g")?));
    assert_eq!(profile.fat.total, None);

    // A nutrient present without an amount reports nothing, and a record
    // reporting nothing has no profile at all.
    let no_amount = foundation_record(&[nutrient_without_amount(1003, "g")]);
    let (result, foods) = collect(no_amount.as_bytes(), 1);
    result?;
    let food = first_food(&foods)?;
    assert_eq!(food.parsed.nutrition, None);
    Ok(())
}

#[test]
fn keeps_negative_source_amounts() -> Result<(), Box<dyn StdError>> {
    // Foundation publishes negative carbohydrate-by-difference values for
    // some meats; the source amount is kept as reported.
    let input = foundation_record(&[nutrient(1005, "g", "-0.475")]);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    assert_eq!(
        nutrition_of(first_food(&foods)?)?
            .carbohydrates
            .including_fibre,
        Some(Mass::parse("-0.475 g")?)
    );
    Ok(())
}

#[test]
fn parses_exponent_notation_amounts_from_the_archive() -> Result<(), Box<dyn StdError>> {
    let input = foundation_record(&[nutrient(1093, "mg", "4.10E+3")]);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    assert_eq!(
        nutrition_of(first_food(&foods)?)?.sodium,
        Some(Mass::parse("4100 mg")?)
    );
    Ok(())
}

#[test]
fn rejects_amounts_beyond_the_decimal_precision() -> Result<(), Box<dyn StdError>> {
    let input = foundation_record(&[nutrient(1004, "g", "0.12345678901234567890123456789")]);
    match single_record_error(input)? {
        RecordError::NutrientAmount { nutrient_id, .. } => assert_eq!(nutrient_id, 1004),
        other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
    Ok(())
}

#[test]
fn rejects_covered_nutrients_in_unsupported_units() -> Result<(), Box<dyn StdError>> {
    let input = foundation_record(&[nutrient(1004, "IU", "100")]);
    match single_record_error(input)? {
        RecordError::NutrientUnit { nutrient_id, unit } => {
            assert_eq!(nutrient_id, 1004);
            assert_eq!(unit, "IU");
        }
        other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
    Ok(())
}

#[test]
fn rejects_duplicate_covered_nutrients() -> Result<(), Box<dyn StdError>> {
    let input = foundation_record(&[nutrient(1003, "g", "7.0"), nutrient(1003, "g", "8.0")]);
    match single_record_error(input)? {
        RecordError::DuplicateNutrient { nutrient_id } => assert_eq!(nutrient_id, 1003),
        other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
    Ok(())
}

#[test]
fn uncovered_nutrients_leave_no_profile() -> Result<(), Box<dyn StdError>> {
    let input = foundation_record(&[nutrient(1120, "µg", "3.0")]);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    assert_eq!(first_food(&foods)?.parsed.nutrition, None);
    Ok(())
}

#[test]
fn parses_foundation_portions_into_servings() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[
            r#"{"fdcId":1,"description":"Cheese","dataType":"Foundation","foodPortions":[
            {"id":1,"value":2.0,"measureUnit":{"id":1001,"name":"tablespoon","abbreviation":"tbsp"},"modifier":"","gramWeight":33.9,"amount":2.0},
            {"id":2,"value":1.0,"measureUnit":{"id":1002,"name":"cup"},"modifier":"drained","gramWeight":164,"amount":1.0},
            {"id":3,"value":0.5,"measureUnit":{"id":1002,"name":"cup"},"gramWeight":41},
            {"id":4,"value":1.0,"measureUnit":{"id":1004,"name":"cup"},"portionDescription":"shredded","gramWeight":28},
            {"id":5,"value":1.0,"measureUnit":{"id":1001,"name":"tablespoon"},"gramWeight":0},
            {"id":6,"value":1.0,"measureUnit":{"id":1001,"name":"tablespoon"}},
            {"id":7,"measureUnit":{"id":1001,"name":"tablespoon"},"gramWeight":12}
        ]}"#,
        ],
    );
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    let food = first_food(&foods)?;
    assert_eq!(
        food.parsed.serving_sizes,
        vec![
            serving("2 tablespoon", "33.9 g")?,
            serving("1 cup, drained", "164 g")?,
            serving("0.5 cup", "41 g")?,
            serving("1 cup, shredded", "28 g")?,
        ]
    );
    Ok(())
}

#[test]
fn parses_sr_legacy_portions_from_their_modifiers() -> Result<(), Box<dyn StdError>> {
    // SR Legacy portions carry their description in `modifier`; the measure
    // unit is the placeholder `undetermined` for every portion.
    let input = document(
        "SRLegacyFoods",
        &[
            r#"{"fdcId":167512,"description":"Biscuits","dataType":"SR Legacy","foodPortions":[
            {"id":1,"value":1.0,"amount":1.0,"measureUnit":{"id":9999,"name":"undetermined","abbreviation":"undetermined"},"modifier":"serving 1 roll with icing","gramWeight":44.0,"sequenceNumber":1},
            {"id":2,"value":1.0,"amount":1.0,"measureUnit":{"id":9999,"name":"undetermined","abbreviation":"undetermined"},"modifier":"serving","gramWeight":30,"sequenceNumber":2}
        ]}"#,
        ],
    );
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    assert_eq!(
        first_food(&foods)?.parsed.serving_sizes,
        vec![
            serving("serving 1 roll with icing", "44.0 g")?,
            serving("serving", "30 g")?,
        ]
    );
    Ok(())
}

#[test]
fn parses_survey_portions_and_skips_unquantified_ones() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "SurveyFoods",
        &[
            r#"{"fdcId":2705383,"description":"Milk, human","dataType":"Survey (FNDDS)","foodNutrients":[],"foodPortions":[
            {"id":290506,"measureUnit":{"id":9999,"name":"undetermined","abbreviation":"undetermined"},"modifier":"10205","portionDescription":"1 cup","gramWeight":246,"sequenceNumber":1},
            {"id":290507,"measureUnit":{"id":9999,"name":"undetermined","abbreviation":"undetermined"},"modifier":"90000","portionDescription":"Quantity not specified","gramWeight":0,"sequenceNumber":2},
            {"id":290508,"measureUnit":{"id":9999,"name":"undetermined","abbreviation":"undetermined"},"modifier":"30000","portionDescription":"1 fl oz","gramWeight":30.800000000000000710542735760100185871124267578125,"sequenceNumber":3},
            {"id":290509,"measureUnit":{"id":9999,"name":"undetermined","abbreviation":"undetermined"},"modifier":"90001","portionDescription":"  ","gramWeight":100,"sequenceNumber":4}
        ]}"#,
        ],
    );
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    let food = first_food(&foods)?;
    assert_eq!(food.parsed.nutrition, None);
    // The binary-float gram weight still defines its serving under the
    // measured-quantity rounding policy.
    assert_eq!(
        food.parsed.serving_sizes,
        vec![serving("1 cup", "246 g")?, serving("1 fl oz", "30.8 g")?]
    );
    Ok(())
}

#[test]
fn stores_one_profile_for_foods_with_many_servings() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[
            r#"{"fdcId":1,"description":"Bread","dataType":"Foundation","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":7.0}],"foodPortions":[
            {"id":1,"value":1.0,"measureUnit":{"id":1100,"name":"slice"},"gramWeight":30},
            {"id":2,"value":2.0,"measureUnit":{"id":1100,"name":"slice"},"gramWeight":60}
        ]}"#,
        ],
    );
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    let food = first_food(&foods)?;
    // Servings store no nutrition of their own; the profile stays singular.
    assert_eq!(food.parsed.serving_sizes.len(), 2);
    let profile = nutrition_of(food)?;
    assert_eq!(profile.protein, Some(Mass::parse("7.0 g")?));
    Ok(())
}

#[test]
fn normalizes_branded_label_nutrients_to_the_basis() -> Result<(), Box<dyn StdError>> {
    // Label values are per serving; this record divides evenly, so the
    // normalized amounts are exact.
    let record = r#"{"fdcId":565809,"description":"GRANOLA","dataType":"Branded","servingSize":25,"servingSizeUnit":"g","householdServingFullText":"0.25 cup","labelNutrients":{"calories":{"value":200},"fat":{"value":3.0},"saturatedFat":{"value":1.0},"transFat":{"value":0.0},"cholesterol":{"value":5.0},"sodium":{"value":75.0},"carbohydrates":{"value":12.5},"fiber":{"value":2.0},"sugars":{"value":3.0},"addedSugar":{"value":1.0},"protein":{"value":2.5}}}"#;
    let input = document("BrandedFoods", &[record]);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    let food = first_food(&foods)?;
    let profile = match food.parsed.nutrition.as_ref() {
        Some(profile) => profile,
        None => return Err(io::Error::other("branded nutrition is missing").into()),
    };

    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(profile.energy, Some(Energy::parse("800 kcal")?));
    assert_eq!(profile.fat.total, Some(Mass::parse("12 g")?));
    assert_eq!(profile.fat.saturated, Some(Mass::parse("4 g")?));
    assert_eq!(profile.fat.trans, Some(Mass::parse("0 g")?));
    assert_eq!(profile.cholesterol, Some(Mass::parse("20 mg")?));
    assert_eq!(profile.sodium, Some(Mass::parse("300 mg")?));
    // US label carbohydrates include fibre.
    assert_eq!(
        profile.carbohydrates.including_fibre,
        Some(Mass::parse("50 g")?)
    );
    assert_eq!(profile.fibre, Some(Mass::parse("8 g")?));
    assert_eq!(
        profile.carbohydrates.sugars.total,
        Some(Mass::parse("12 g")?)
    );
    assert_eq!(
        profile.carbohydrates.sugars.added,
        Some(Mass::parse("4 g")?)
    );
    assert_eq!(profile.protein, Some(Mass::parse("10 g")?));
    assert_eq!(profile.carbohydrates.excluding_fibre, None);
    assert_eq!(profile.carbohydrates.sugars.lactose, None);
    assert_eq!(profile.carbohydrates.starch, None);
    assert_eq!(profile.fat.monounsaturated, None);
    assert_eq!(profile.fat.polyunsaturated, None);
    assert_eq!(profile.salt, None);
    Ok(())
}

#[test]
fn branded_volume_servings_use_the_millilitre_basis() -> Result<(), Box<dyn StdError>> {
    let record = r#"{"fdcId":2,"description":"Soda","dataType":"Branded","servingSize":250,"servingSizeUnit":"ml","householdServingFullText":"1 portion (250 ml)","labelNutrients":{"calories":{"value":100},"sodium":{"value":25.0}}}"#;
    let input = document("BrandedFoods", &[record]);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;
    let food = first_food(&foods)?;
    let profile = match food.parsed.nutrition.as_ref() {
        Some(profile) => profile,
        None => return Err(io::Error::other("branded nutrition is missing").into()),
    };
    assert_eq!(profile.per, NutritionBasis::Per100Millilitres);
    assert_eq!(profile.energy, Some(Energy::parse("40 kcal")?));
    assert_eq!(profile.sodium, Some(Mass::parse("10 mg")?));
    assert_eq!(
        food.parsed.serving_sizes,
        vec![serving("1 portion (250 ml)", "250 ml")?]
    );
    Ok(())
}

#[test]
fn branded_records_without_serving_evidence_carry_no_nutrition() -> Result<(), Box<dyn StdError>> {
    let cases = [
        // Labels without a serving size have no basis evidence.
        r#"{"fdcId":1,"description":"A","dataType":"Branded","labelNutrients":{"calories":{"value":100}}}"#,
        // A serving size in an unsupported unit defines no basis.
        r#"{"fdcId":2,"description":"B","dataType":"Branded","servingSize":28,"servingSizeUnit":"oz","labelNutrients":{"calories":{"value":100}}}"#,
        // A nonpositive serving size defines no basis.
        r#"{"fdcId":3,"description":"C","dataType":"Branded","servingSize":0,"servingSizeUnit":"g","labelNutrients":{"calories":{"value":100}}}"#,
        // A serving size without a unit defines no basis.
        r#"{"fdcId":4,"description":"D","dataType":"Branded","servingSize":28,"labelNutrients":{"calories":{"value":100}}}"#,
    ];

    for record in cases {
        let input = document("BrandedFoods", &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        result?;
        let food = first_food(&foods)?;
        assert_eq!(
            food.parsed.nutrition, None,
            "record was given nutrition: {record}"
        );
        assert!(
            food.parsed.serving_sizes.is_empty(),
            "record was given a serving: {record}"
        );
    }
    Ok(())
}

#[test]
fn branded_servings_survive_without_household_text_or_labels() -> Result<(), Box<dyn StdError>> {
    let cases = [
        r#"{"fdcId":1,"description":"A","dataType":"Branded","servingSize":28,"servingSizeUnit":"g"}"#,
        r#"{"fdcId":2,"description":"B","dataType":"Branded","servingSize":28,"servingSizeUnit":"g","householdServingFullText":"  "}"#,
    ];

    for record in cases {
        let input = document("BrandedFoods", &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        result?;
        let food = first_food(&foods)?;
        // The serving exists without nutrition, and is named by itself.
        assert_eq!(food.parsed.nutrition, None);
        assert_eq!(food.parsed.serving_sizes, vec![serving("serving", "28 g")?]);
    }
    Ok(())
}

#[test]
fn rejects_unrepresentable_branded_label_values() -> Result<(), Box<dyn StdError>> {
    let record = r#"{"fdcId":1,"description":"A","dataType":"Branded","servingSize":28,"servingSizeUnit":"g","labelNutrients":{"calories":{"value":0.12345678901234567890123456789}}}"#;
    let input = document("BrandedFoods", &[record]);
    match single_record_error(input)? {
        RecordError::LabelNutrient { label, .. } => assert_eq!(label, "calories"),
        other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
    Ok(())
}

#[test]
fn allows_missing_and_null_categories() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[
            r#"{"fdcId":1,"description":"No category","dataType":"Foundation"}"#,
            r#"{"fdcId":2,"description":"Null category","dataType":"Foundation","foodCategory":null}"#,
        ],
    );

    let (result, foods) = collect(input.as_bytes(), 1);
    result?;

    assert_eq!(foods.len(), 2);
    for food in &foods {
        assert_eq!(food.category, None);
    }
    Ok(())
}

#[test]
fn accepts_empty_recognized_array() -> Result<(), Box<dyn StdError>> {
    let (result, foods) = collect(br#"{"FoundationFoods":[]}"#, 1);
    result?;
    assert!(foods.is_empty());
    Ok(())
}

#[test]
fn accepts_trailing_whitespace() -> Result<(), Box<dyn StdError>> {
    let (result, foods) = collect(b"{\"FoundationFoods\":[]}\n\t ", 1);
    result?;
    assert!(foods.is_empty());
    Ok(())
}

#[test]
fn rejects_empty_input_and_empty_object() -> Result<(), Box<dyn StdError>> {
    for input in [&b""[..], &b"{}"[..]] {
        let (result, foods) = collect(input, 1);
        assert!(foods.is_empty());
        let error = error(result)?;
        assert!(
            matches!(&error, ParseError::Document { .. }),
            "unexpected error: {error}"
        );
    }
    Ok(())
}

#[test]
fn rejects_unknown_multiple_and_trailing_documents() -> Result<(), Box<dyn StdError>> {
    let cases = [
        (
            &br#"{"OtherFoods":[]}"#[..],
            "unsupported root",
            "OtherFoods",
        ),
        (
            &br#"{"FoundationFoods":[],"BrandedFoods":[]}"#[..],
            "multiple roots",
            "another entry",
        ),
        (
            &br#"{"FoundationFoods":[]} trailing"#[..],
            "trailing content",
            "trailing",
        ),
        (
            &br#"{"FoundationFoods":{}}"#[..],
            "non-array root",
            "invalid type",
        ),
    ];

    for (input, label, message) in cases {
        let (result, foods) = collect(input, 1);
        assert!(foods.is_empty(), "{label} produced food records");
        let error = error(result)?;
        assert!(
            matches!(&error, ParseError::Document { .. }),
            "{label} produced the wrong error: {error}"
        );
        assert!(
            error.to_string().contains(message),
            "{label} message did not contain {message:?}: {error}"
        );
    }
    Ok(())
}

#[test]
fn rejects_malformed_json() -> Result<(), Box<dyn StdError>> {
    let (result, foods) = collect(br#"{"FoundationFoods":[{"fdcId":1,}]}"#, 1);
    assert!(foods.is_empty());
    let error = error(result)?;
    match error {
        ParseError::Record {
            position,
            source: RecordError::Syntax(source),
        } => {
            assert_eq!(position.get(), 1);
            assert!(source.line() > 0);
            assert!(source.column() > 0);
        }
        other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
    Ok(())
}

#[test]
fn reports_io_failure_while_reading_a_later_record() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[IO_FAILURE_FIRST_RECORD, IO_FAILURE_SECOND_RECORD],
    );
    let second_start = record_offset(&input, IO_FAILURE_SECOND_RECORD)?;
    let fail_after = second_start + IO_FAILURE_SECOND_RECORD.len() / 2;
    let reader = FailingReader::new(input.as_bytes(), fail_after);

    let (result, foods) = collect_reader(reader, 1);

    assert_eq!(
        foods.len(),
        1,
        "the valid preceding record was not delivered"
    );
    let delivered = first_food(&foods)?;
    assert_eq!(delivered.fdc_id.as_str(), "1");

    let error = error(result)?;
    let displayed = error.to_string();
    assert!(
        displayed.contains("failed to read record JSON: simulated input read failure"),
        "unexpected read error label: {displayed}"
    );
    assert!(
        !displayed.contains("malformed record JSON"),
        "I/O failure was labeled as malformed JSON: {displayed}"
    );
    match error {
        ParseError::Record {
            position,
            source: RecordError::Read(source),
        } => {
            assert_eq!(position.get(), 2);
            assert_eq!(source.io_error_kind(), Some(io::ErrorKind::Other));
            assert!(source.to_string().contains("simulated input read failure"));
        }
        other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
    Ok(())
}

#[test]
fn reports_io_failure_while_reading_document_framing() -> Result<(), Box<dyn StdError>> {
    let input = br#"{"FoundationFoods":["#;
    let reader = FailingReader::new(input, 5);

    let (result, foods) = collect_reader(reader, 1);

    assert!(foods.is_empty());
    let error = error(result)?;
    require_document_read_error(&error)?;
    Ok(())
}

#[test]
fn reports_io_failure_while_checking_trailing_input() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[IO_FAILURE_FIRST_RECORD, IO_FAILURE_SECOND_RECORD],
    );
    let reader = FailingReader::new(input.as_bytes(), input.len());

    let (result, foods) = collect_reader(reader, 1);

    assert_eq!(foods.len(), 2, "completed records were not delivered");
    let first = first_food(&foods)?;
    assert_eq!(first.fdc_id.as_str(), "1");
    let second = foods
        .get(1)
        .ok_or_else(|| io::Error::other("second food is missing"))?;
    assert_eq!(second.fdc_id.as_str(), "2");

    let error = error(result)?;
    require_document_read_error(&error)?;
    Ok(())
}

#[test]
fn rejects_missing_header_fields_with_json_position() -> Result<(), Box<dyn StdError>> {
    let records = [
        r#"{"description":"Food","dataType":"Foundation"}"#,
        r#"{"fdcId":1,"dataType":"Foundation"}"#,
        r#"{"fdcId":1,"description":"Food"}"#,
    ];

    for record in records {
        let input = document("FoundationFoods", &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        assert!(foods.is_empty());
        let error = error(result)?;
        match error {
            ParseError::Record {
                position,
                source: RecordError::Header(source),
            } => {
                assert_eq!(position.get(), 1);
                assert!(source.line() > 0);
                assert!(source.column() > 0);
            }
            other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
        }
    }
    Ok(())
}

#[test]
fn rejects_wrong_header_field_types() -> Result<(), Box<dyn StdError>> {
    let records = [
        r#"{"fdcId":"1","description":"Food","dataType":"Foundation"}"#,
        r#"{"fdcId":-1,"description":"Food","dataType":"Foundation"}"#,
        r#"{"fdcId":1.5,"description":"Food","dataType":"Foundation"}"#,
        r#"{"fdcId":1,"description":42,"dataType":"Foundation"}"#,
        r#"{"fdcId":1,"description":"Food","dataType":42}"#,
        r#"{"fdcId":1,"description":"Food","dataType":"Sample"}"#,
    ];

    for record in records {
        let input = document("FoundationFoods", &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        assert!(foods.is_empty());
        let error = error(result)?;
        assert!(
            matches!(
                error,
                ParseError::Record {
                    source: RecordError::Header(_),
                    ..
                }
            ),
            "wrong-type record did not produce a header error"
        );
    }
    Ok(())
}

#[test]
fn rejects_positional_array_record() -> Result<(), Box<dyn StdError>> {
    let input = document("FoundationFoods", &[r#"[123,"Apple","Foundation"]"#]);
    let (result, foods) = collect(input.as_bytes(), 1);
    assert!(foods.is_empty());
    let error = error(result)?;

    assert!(
        matches!(
            error,
            ParseError::Record {
                position,
                source: RecordError::NotObject,
            } if position.get() == 1
        ),
        "positional array was accepted as a food record"
    );
    Ok(())
}

#[test]
fn rejects_zero_fdc_id() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[r#"{"fdcId":0,"description":"Food","dataType":"Foundation"}"#],
    );
    let (result, foods) = collect(input.as_bytes(), 1);
    assert!(foods.is_empty());
    let error = error(result)?;

    assert!(
        matches!(
            error,
            ParseError::Record {
                position,
                source: RecordError::ZeroFdcId,
            } if position.get() == 1
        ),
        "zero fdcId was not rejected"
    );
    Ok(())
}

#[test]
fn rejects_blank_description_with_record_position() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[
            FOUNDATION_RECORD_NULL_CATEGORY,
            r#"{"fdcId":3,"description":" \t ","dataType":"Foundation"}"#,
        ],
    );
    let (result, foods) = collect(input.as_bytes(), 1);
    assert_eq!(foods.len(), 1);
    let error = error(result)?;

    assert!(
        matches!(
            error,
            ParseError::Record {
                position,
                source: RecordError::BlankDescription,
            } if position.get() == 2
        ),
        "blank description was not rejected at record two"
    );
    Ok(())
}

#[test]
fn rejects_root_and_data_type_mismatch() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[r#"{"fdcId":1,"description":"Food","dataType":"Branded"}"#],
    );
    let (result, foods) = collect(input.as_bytes(), 1);
    assert!(foods.is_empty());
    let error = error(result)?;

    match error {
        ParseError::Record {
            position,
            source: RecordError::KindMismatch { root, data_type },
        } => {
            assert_eq!(position.get(), 1);
            assert_eq!(root, FoodKind::Foundation);
            assert_eq!(data_type, FoodKind::Branded);
        }
        other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
    Ok(())
}

#[test]
fn rejects_malformed_category_types() -> Result<(), Box<dyn StdError>> {
    let cases = [
        (
            "FoundationFoods",
            r#"{"fdcId":1,"description":"Food","dataType":"Foundation","foodCategory":"Legumes"}"#,
        ),
        (
            "FoundationFoods",
            r#"{"fdcId":1,"description":"Food","dataType":"Foundation","foodCategory":[]}"#,
        ),
        (
            "FoundationFoods",
            r#"{"fdcId":1,"description":"Food","dataType":"Foundation","foodCategory":{"description":42}}"#,
        ),
        (
            "BrandedFoods",
            r#"{"fdcId":1,"description":"Food","dataType":"Branded","brandedFoodCategory":42}"#,
        ),
    ];

    for (root, record) in cases {
        let input = document(root, &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        assert!(foods.is_empty(), "record was accepted: {record}");
        let error = error(result)?;
        assert!(
            matches!(
                error,
                ParseError::Record {
                    source: RecordError::Header(_),
                    ..
                }
            ),
            "malformed category did not produce a header error"
        );
    }
    Ok(())
}

#[test]
fn reports_consumer_disconnect() -> Result<(), Box<dyn StdError>> {
    let input = document("FoundationFoods", &[FOUNDATION_RECORD_NULL_CATEGORY]);
    let (sender, receiver) = mpsc::channel::<PreparedFood>(1);
    drop(receiver);

    let result = stream_foods(input.as_bytes(), &sender);
    let error = error(result)?;

    assert!(
        matches!(
            error,
            ParseError::ConsumerDisconnected { position } if position.get() == 1
        ),
        "consumer disconnect was not reported"
    );
    Ok(())
}

#[test]
fn streams_large_input_to_incremental_consumer() -> Result<(), Box<dyn StdError>> {
    let input = generated_foundation_json(5_000);
    let (sender, mut receiver) = mpsc::channel::<PreparedFood>(1);

    let consumer = thread::spawn(move || {
        let first = receiver.blocking_recv();
        drop(receiver);
        first
    });

    let mut reader = CountingReader::new(input.as_bytes());
    let result = stream_foods(&mut reader, &sender);
    drop(sender);

    let consumer_result = consumer.join();
    assert!(consumer_result.is_ok(), "consumer thread panicked");
    let first = consumer_result.ok().flatten();
    assert!(first.is_some(), "consumer received no record");
    let bytes_read = reader.bytes_read();

    let error = error(result)?;
    assert!(
        matches!(error, ParseError::ConsumerDisconnected { .. }),
        "early receiver drop was not reported"
    );
    assert!(
        bytes_read < input.len() / 2,
        "parsed {bytes_read} of {} bytes before the receiver disappeared",
        input.len()
    );
    Ok(())
}

#[test]
fn streams_all_records_from_large_input() -> Result<(), Box<dyn StdError>> {
    let input = generated_foundation_json(2_000);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;

    assert_eq!(foods.len(), 2_000);
    let last = foods
        .last()
        .ok_or_else(|| io::Error::other("missing last food"))?;
    assert_eq!(last.position.get(), 2_000);
    assert_eq!(last.fdc_id.as_str(), "2000");
    Ok(())
}

impl FdcId {
    /// Returns the identifier as text.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl FoodDescription {
    /// Returns the description as text.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl FoodCategory {
    /// Returns the category as text.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl RecordPosition {
    /// Returns the position as a one-based number.
    const fn get(self) -> usize {
        self.0.get()
    }
}

fn document(root: &str, records: &[&str]) -> String {
    let mut input = String::from("{\"");
    input.push_str(root);
    input.push_str("\":[");
    input.push_str(&records.join(","));
    input.push_str("]}");
    input
}

fn collect(input: &[u8], capacity: usize) -> (Result<(), ParseError>, Vec<PreparedFood>) {
    collect_reader(input, capacity)
}

fn collect_reader(
    input: impl Read,
    capacity: usize,
) -> (Result<(), ParseError>, Vec<PreparedFood>) {
    let (sender, mut receiver) = mpsc::channel(capacity);
    let consumer = thread::spawn(move || {
        let mut foods = Vec::new();
        while let Some(food) = receiver.blocking_recv() {
            foods.push(food);
        }
        foods
    });

    let result = stream_foods(input, &sender);
    drop(sender);

    let consumer_result = consumer.join();
    assert!(consumer_result.is_ok(), "consumer thread panicked");
    let foods = consumer_result.unwrap_or_default();
    (result, foods)
}

fn record_offset(input: &str, record: &str) -> Result<usize, io::Error> {
    input
        .find(record)
        .ok_or_else(|| io::Error::other("record is missing from the test document"))
}

fn error(result: Result<(), ParseError>) -> Result<ParseError, io::Error> {
    result
        .err()
        .ok_or_else(|| io::Error::other("input was accepted"))
}

fn require_document_read_error(error: &ParseError) -> Result<(), io::Error> {
    let displayed = error.to_string();
    if displayed.contains("invalid USDA FoodData Central document") {
        return Err(io::Error::other(format!(
            "I/O failure was labeled as an invalid document: {displayed}"
        )));
    }
    if !displayed.contains("failed to read USDA FoodData Central document:") {
        return Err(io::Error::other(format!(
            "unexpected document read error label: {displayed}"
        )));
    }
    match error {
        ParseError::Read { source } => {
            let kind = source.io_error_kind();
            if kind != Some(io::ErrorKind::Other) {
                return Err(io::Error::other(format!(
                    "unexpected I/O error kind: {kind:?}"
                )));
            }
            if !source.to_string().contains("simulated input read failure") {
                return Err(io::Error::other(format!(
                    "unexpected I/O error message: {source}"
                )));
            }
            Ok(())
        }
        other => Err(io::Error::other(format!("unexpected error: {other}"))),
    }
}

fn first_food(foods: &[PreparedFood]) -> Result<&PreparedFood, io::Error> {
    foods
        .first()
        .ok_or_else(|| io::Error::other("no food records"))
}

fn nutrition_of(food: &PreparedFood) -> Result<&NutritionProfile, Box<dyn StdError>> {
    food.parsed
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("nutrition profile is absent").into())
}

fn serving(name: &str, quantity: &str) -> Result<ServingSize, Box<dyn StdError>> {
    Ok(ServingSize {
        name: ServingName::new(name)?,
        quantity: MeasuredQuantity::parse(quantity)?,
    })
}

/// Builds a `foodNutrients` entry with a covered nutrient's identity and amount.
fn nutrient(nutrient_id: u64, unit: &str, amount: &str) -> String {
    format!(r#"{{"nutrient":{{"id":{nutrient_id},"unitName":"{unit}"}},"amount":{amount}}}"#)
}

/// Builds a `foodNutrients` entry that reports no amount.
fn nutrient_without_amount(nutrient_id: u64, unit: &str) -> String {
    format!(r#"{{"nutrient":{{"id":{nutrient_id},"unitName":"{unit}"}}}}"#)
}

/// Builds a Foundation document carrying one record with the given nutrients.
fn foundation_record(nutrients: &[String]) -> String {
    document(
        "FoundationFoods",
        &[format!(
            r#"{{"fdcId":1,"description":"Test food","dataType":"Foundation","foodNutrients":[{}]}}"#,
            nutrients.join(",")
        )
        .as_str()],
    )
}

/// Collects a one-record document and requires it to fail with a record error.
fn single_record_error(input: String) -> Result<RecordError, Box<dyn StdError>> {
    let (result, foods) = collect(input.as_bytes(), 1);
    assert!(foods.is_empty(), "invalid record was accepted");
    match error(result)? {
        ParseError::Record { source, .. } => Ok(source),
        other => Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
}

fn generated_foundation_json(count: usize) -> String {
    let mut json = String::from("{\"FoundationFoods\":[");
    for id in 1..=count {
        if id > 1 {
            json.push(',');
        }
        json.push_str("{\"fdcId\":");
        json.push_str(&id.to_string());
        json.push_str(",\"description\":\"Food ");
        json.push_str(&id.to_string());
        json.push_str("\",\"dataType\":\"Foundation\",\"foodCategory\":{\"description\":\"Test\"},\"foodNutrients\":[{\"amount\":1.00}]}");
    }
    json.push_str("]}");
    json
}

struct CountingReader<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> CountingReader<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    const fn bytes_read(&self) -> usize {
        self.offset
    }
}

impl Read for CountingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = match self.input.get(self.offset..) {
            Some(remaining) => remaining,
            None => &[],
        };
        let count = remaining.len().min(buffer.len());
        let source = remaining
            .get(..count)
            .ok_or_else(|| io::Error::other("source range was out of bounds"))?;
        let destination = buffer
            .get_mut(..count)
            .ok_or_else(|| io::Error::other("buffer range was out of bounds"))?;
        destination.copy_from_slice(source);
        self.offset = self
            .offset
            .checked_add(count)
            .ok_or_else(|| io::Error::other("read offset overflowed"))?;
        Ok(count)
    }
}

struct FailingReader<'a> {
    input: &'a [u8],
    offset: usize,
    fail_after: usize,
}

impl<'a> FailingReader<'a> {
    const fn new(input: &'a [u8], fail_after: usize) -> Self {
        Self {
            input,
            offset: 0,
            fail_after,
        }
    }
}

impl Read for FailingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.offset >= self.fail_after {
            return Err(io::Error::other("simulated input read failure"));
        }
        let remaining = match self.input.get(self.offset..) {
            Some(remaining) => remaining,
            None => &[],
        };
        let available = self.fail_after - self.offset;
        let count = remaining.len().min(buffer.len()).min(available);
        let source = remaining
            .get(..count)
            .ok_or_else(|| io::Error::other("source range was out of bounds"))?;
        let destination = buffer
            .get_mut(..count)
            .ok_or_else(|| io::Error::other("buffer range was out of bounds"))?;
        destination.copy_from_slice(source);
        self.offset = self
            .offset
            .checked_add(count)
            .ok_or_else(|| io::Error::other("read offset overflowed"))?;
        Ok(count)
    }
}
