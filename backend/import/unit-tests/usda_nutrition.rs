use std::error::Error;
use std::io;

use super::*;
use crumbs_database::nutrition_model::{NutritionBasis, Qualifier, Reported};

fn parse_data(input: &str, kind: FoodKind) -> Result<FoodData, NutritionError> {
    super::parse_food_data(input, kind).map(|parsed| parsed.food_data)
}

#[test]
fn maps_all_supported_usda_nutrient_categories() -> Result<(), Box<dyn Error>> {
    let input = r#"{
      "foodNutrients": [
        {"nutrient":{"id":1003,"unitName":"g"},"amount":7},
        {"nutrient":{"id":1004,"unitName":"g"},"amount":8},
        {"nutrient":{"id":1005,"unitName":"g"},"amount":9},
        {"nutrient":{"id":1009,"unitName":"mg"},"amount":10},
        {"nutrient":{"id":1013,"unitName":"g"},"amount":11},
        {"nutrient":{"id":1063,"unitName":"mg"},"amount":12},
        {"nutrient":{"id":2000,"unitName":"g"},"amount":13},
        {"nutrient":{"id":1079,"unitName":"g"},"amount":14},
        {"nutrient":{"id":1093,"unitName":"µg"},"amount":15},
        {"nutrient":{"id":1235,"unitName":"g"},"amount":16},
        {"nutrient":{"id":1253,"unitName":"mg"},"amount":17},
        {"nutrient":{"id":1257,"unitName":"g"},"amount":18},
        {"nutrient":{"id":1258,"unitName":"g"},"amount":19},
        {"nutrient":{"id":1292,"unitName":"g"},"amount":20},
        {"nutrient":{"id":1293,"unitName":"g"},"amount":21}
      ]
    }"#;
    let data = parse_data(input, FoodKind::Foundation)?;
    let profile = data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("nutrition profile is missing"))?;
    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(
        amount_text(&profile.nutrients.protein),
        Some("7 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.fat.total),
        Some("8 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.carbohydrates.including_fibre),
        Some("9 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.carbohydrates.starch),
        Some("10 mg".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.carbohydrates.sugars.lactose),
        Some("11 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.carbohydrates.sugars.total),
        Some("12 mg".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.fibre),
        Some("14 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.sodium),
        Some("15 µg".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.carbohydrates.sugars.added),
        Some("16 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.cholesterol),
        Some("17 mg".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.fat.trans),
        Some("18 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.fat.saturated),
        Some("19 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.fat.monounsaturated),
        Some("20 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.fat.polyunsaturated),
        Some("21 g".to_owned())
    );
    assert_eq!(profile.nutrients.carbohydrates.excluding_fibre, None);
    Ok(())
}

#[test]
fn supports_each_usda_food_kind_and_branded_mass_or_volume_basis() -> Result<(), Box<dyn Error>> {
    for (kind, data_type) in [
        (FoodKind::Foundation, "Foundation"),
        (FoodKind::SrLegacy, "SR Legacy"),
        (FoodKind::Survey, "Survey (FNDDS)"),
    ] {
        let input = format!(
            r#"{{"foodNutrients":[{{"nutrient":{{"id":1003,"unitName":"g"}},"amount":1}}],"dataType":"{data_type}"}}"#
        );
        let data = parse_data(&input, kind)?;
        assert_eq!(
            data.nutrition.as_ref().map(|profile| profile.per),
            Some(NutritionBasis::Per100Grams)
        );
    }

    let mass = parse_data(
        r#"{"servingSize":30,"servingSizeUnit":"G","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1}]}"#,
        FoodKind::Branded,
    )?;
    assert_eq!(
        mass.nutrition.as_ref().map(|profile| profile.per),
        Some(NutritionBasis::Per100Grams)
    );

    let volume = parse_data(
        r#"{"servingSize":250,"servingSizeUnit":"mL","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1}]}"#,
        FoodKind::Branded,
    )?;
    assert_eq!(
        volume.nutrition.as_ref().map(|profile| profile.per),
        Some(NutritionBasis::Per100Millilitres)
    );
    let serving = volume
        .serving_sizes
        .first()
        .ok_or_else(|| io::Error::other("branded measured serving is missing"))?;
    assert_eq!(serving.quantity().to_string(), "250 ml");
    let unsupported_basis = super::parse_food_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1}]}"#,
        FoodKind::Branded,
    )?;
    assert_eq!(unsupported_basis.food_data.nutrition, None);
    assert!(!unsupported_basis.sanitizations.is_empty());

    let missing_serving_unit = super::parse_food_data(
        r#"{"servingSize":30,"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1}]}"#,
        FoodKind::Branded,
    )?;
    assert_eq!(missing_serving_unit.food_data.nutrition, None);
    assert!(missing_serving_unit.food_data.serving_sizes.is_empty());
    assert!(
        missing_serving_unit
            .sanitizations
            .iter()
            .any(|entry| entry.field == "serving_sizes")
    );

    let invalid_serving = super::parse_food_data(
        r#"{"servingSize":0,"servingSizeUnit":"g","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1}]}"#,
        FoodKind::Branded,
    )?;
    assert_eq!(
        invalid_serving
            .food_data
            .nutrition
            .as_ref()
            .map(|profile| profile.per),
        Some(NutritionBasis::Per100Grams)
    );
    assert!(invalid_serving.food_data.serving_sizes.is_empty());
    assert!(!invalid_serving.sanitizations.is_empty());
    Ok(())
}

#[test]
fn absent_or_empty_nutrient_arrays_produce_no_profile() -> Result<(), Box<dyn Error>> {
    for input in [
        r#"{}"#,
        r#"{"foodNutrients":null}"#,
        r#"{"foodNutrients":[]}"#,
    ] {
        let data = parse_data(input, FoodKind::Foundation)?;
        assert_eq!(data.nutrition, None);
    }
    Ok(())
}

#[test]
fn selects_energy_in_project_priority_and_falls_through_missing_candidates()
-> Result<(), Box<dyn Error>> {
    let input = r#"{"foodNutrients":[
      {"nutrient":{"id":1062,"unitName":"kJ"},"amount":40},
      {"nutrient":{"id":1008,"unitName":"kcal"},"amount":30},
      {"nutrient":{"id":2047,"unitName":"kcal"},"amount":20},
      {"nutrient":{"id":2048,"unitName":"kcal"},"amount":10}
    ]}"#;
    let data = parse_data(input, FoodKind::Foundation)?;
    let profile = data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("energy profile is missing"))?;
    assert_eq!(
        profile.energy.as_ref().map(ToString::to_string),
        Some("10 kcal".to_owned())
    );

    for (nutrients, expected) in [
        (
            r#"[{"nutrient":{"id":2048,"unitName":"kcal"},"amount":null},{"nutrient":{"id":2047,"unitName":"kcal"},"amount":20}]"#,
            "20 kcal",
        ),
        (
            r#"[{"nutrient":{"id":2048,"unitName":"kcal"},"amount":null},{"nutrient":{"id":2047,"unitName":"kcal"},"amount":null},{"nutrient":{"id":1008,"unitName":"kcal"},"amount":30}]"#,
            "30 kcal",
        ),
        (
            r#"[{"nutrient":{"id":2048,"unitName":"kcal"},"amount":null},{"nutrient":{"id":2047,"unitName":"kcal"},"amount":null},{"nutrient":{"id":1008,"unitName":"kcal"},"amount":null},{"nutrient":{"id":1062,"unitName":"kJ"},"amount":40}]"#,
            "40 kJ",
        ),
    ] {
        let input = format!(r#"{{"foodNutrients":{nutrients}}}"#);
        let data = parse_data(&input, FoodKind::Foundation)?;
        let actual = data
            .nutrition
            .as_ref()
            .and_then(|profile| profile.energy.as_ref().map(ToString::to_string));
        assert_eq!(actual, Some(expected.to_owned()));
    }

    let no_energy = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":2048,"unitName":"kcal"},"amount":null}]}"#,
        FoodKind::Foundation,
    )?;
    assert_eq!(no_energy.nutrition, None);
    Ok(())
}

#[test]
fn invalid_preferred_energy_is_omitted_and_lower_priority_value_is_used()
-> Result<(), Box<dyn Error>> {
    let input = r#"{"foodNutrients":[
      {"nutrient":{"id":2048,"unitName":"g"},"amount":10},
      {"nutrient":{"id":2047,"unitName":"kcal"},"amount":20}
    ]}"#;
    let parsed = super::parse_food_data(input, FoodKind::Foundation)?;
    let energy = parsed
        .food_data
        .nutrition
        .as_ref()
        .and_then(|profile| profile.energy.as_ref())
        .map(ToString::to_string);
    assert_eq!(energy, Some("20 kcal".to_owned()));
    assert!(!parsed.sanitizations.is_empty());
    Ok(())
}

#[test]
fn sanitizes_negative_nutrients_and_falls_back_from_negative_energy() -> Result<(), Box<dyn Error>>
{
    let input = r#"{"foodNutrients":[
      {"nutrient":{"id":1005,"unitName":"g"},"amount":-0.475},
      {"nutrient":{"id":1003,"unitName":"g"},"amount":"< -0.1"},
      {"nutrient":{"id":2048,"unitName":"kcal"},"amount":-1},
      {"nutrient":{"id":2047,"unitName":"kcal"},"amount":20}
    ]}"#;
    let parsed = super::parse_food_data(input, FoodKind::Foundation)?;
    let profile = parsed
        .food_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("fallback energy profile is missing"))?;
    assert_eq!(profile.nutrients.carbohydrates.including_fibre, None);
    assert_eq!(profile.nutrients.protein, None);
    assert_eq!(
        profile.energy.as_ref().map(ToString::to_string),
        Some("20 kcal".to_owned())
    );
    assert_eq!(parsed.sanitizations.len(), 3);
    assert_eq!(
        parsed.sanitizations[0].field,
        "carbohydrates.including_fibre"
    );
    assert_eq!(parsed.sanitizations[1].field, "protein");
    assert_eq!(parsed.sanitizations[2].field, "energy");
    Ok(())
}

#[test]
fn unavailable_or_negative_loq_does_not_invent_a_bound_or_restore_a_negative_amount()
-> Result<(), Box<dyn Error>> {
    let input = r#"{"foodNutrients":[
      {"nutrient":{"id":1079,"unitName":"g"},"amount":0.2,"loq":0},
      {"nutrient":{"id":1003,"unitName":"g"},"amount":null,"loq":0},
      {"nutrient":{"id":1004,"unitName":"mg"},"amount":3,"loq":-1},
      {"nutrient":{"id":1005,"unitName":"g"},"amount":-0.2,"loq":0.1}
    ]}"#;
    let parsed = super::parse_food_data(input, FoodKind::Foundation)?;
    let profile = parsed
        .food_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("nutrition profile is missing"))?;
    assert_eq!(
        amount_text(&profile.nutrients.fibre),
        Some("0.2 g".to_owned())
    );
    assert_eq!(profile.nutrients.protein, None);
    assert_eq!(
        amount_text(&profile.nutrients.fat.total),
        Some("3 mg".to_owned())
    );
    assert_eq!(profile.nutrients.carbohydrates.including_fibre, None);
    assert_eq!(parsed.sanitizations.len(), 4);
    assert!(
        parsed
            .sanitizations
            .iter()
            .any(|entry| entry.reason.contains("zero or negative limit"))
    );
    assert!(
        parsed
            .sanitizations
            .iter()
            .any(|entry| entry.reason.contains("negative source amount"))
    );
    Ok(())
}

#[test]
fn preserves_exact_decimal_lexemes_and_source_qualifiers() -> Result<(), Box<dyn Error>> {
    let input = r#"{"foodNutrients":[
      {"nutrient":{"id":1003,"unitName":"g"},"amount":0.1000000000000000000000000001},
      {"nutrient":{"id":1093,"unitName":"mg"},"amount":"< 0.5"}
    ]}"#;
    let data = parse_data(input, FoodKind::Foundation)?;
    let profile = data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("nutrition profile is missing"))?;
    assert_eq!(
        amount_text(&profile.nutrients.protein),
        Some("0.1000000000000000000000000001 g".to_owned())
    );
    assert_eq!(
        profile.nutrients.sodium.as_ref().map(Reported::qualifier),
        Some(Qualifier::LessThan)
    );
    assert_eq!(
        amount_text(&profile.nutrients.sodium),
        Some("< 0.5 mg".to_owned())
    );

    let loq = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1079,"unitName":"µg"},"amount":0,"loq":0.55}]}"#,
        FoodKind::Foundation,
    )?;
    let fibre = loq
        .nutrition
        .as_ref()
        .and_then(|profile| profile.nutrients.fibre.as_ref())
        .ok_or_else(|| io::Error::other("LOQ fibre value is missing"))?;
    assert_eq!(fibre.qualifier(), Qualifier::LessThan);
    assert_eq!(fibre.to_string(), "< 0.55 µg");

    let measured = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1079,"unitName":"g"},"amount":0.2,"loq":0.55}]}"#,
        FoodKind::Foundation,
    )?;
    assert_eq!(
        measured
            .nutrition
            .as_ref()
            .and_then(|profile| profile.nutrients.fibre.as_ref())
            .map(ToString::to_string),
        Some("0.2 g".to_owned())
    );
    Ok(())
}

#[test]
fn missing_zero_and_partial_nutrients_remain_distinct() -> Result<(), Box<dyn Error>> {
    let missing = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":null},{"nutrient":{"id":1013,"unitName":"g"},"amount":null}]}"#,
        FoodKind::Foundation,
    )?;
    assert_eq!(missing.nutrition, None);

    let partial = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":0},{"nutrient":{"id":1013,"unitName":"g"},"amount":null}]}"#,
        FoodKind::Foundation,
    )?;
    let profile = partial
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("partial profile is missing"))?;
    assert_eq!(
        amount_text(&profile.nutrients.protein),
        Some("0 g".to_owned())
    );
    assert_eq!(profile.nutrients.carbohydrates.sugars.lactose, None);
    Ok(())
}

#[test]
fn stores_multiple_measured_portions_without_inventing_volume() -> Result<(), Box<dyn Error>> {
    let input = r#"{"foodPortions":[
      {"portionDescription":"one slice","gramWeight":30,"amount":1,"measureUnit":{"name":"slice"}},
      {"portionDescription":"two slices","gramWeight":60,"amount":2,"measureUnit":{"name":"slice"}},
      {"portionDescription":"unmeasured","gramWeight":null}
    ]}"#;
    let data = parse_data(input, FoodKind::Foundation)?;
    assert_eq!(data.nutrition, None);
    assert_eq!(data.serving_sizes.len(), 2);
    assert_eq!(
        data.serving_sizes
            .first()
            .map(|serving| serving.quantity().to_string()),
        Some("30 g".to_owned())
    );
    assert_eq!(
        data.serving_sizes
            .get(1)
            .map(|serving| serving.quantity().to_string()),
        Some("60 g".to_owned())
    );

    let no_volume = parse_data(
        r#"{"foodPortions":[{"portionDescription":"cup","gramWeight":null}]}"#,
        FoodKind::Foundation,
    )?;
    assert!(no_volume.serving_sizes.is_empty());
    Ok(())
}

#[test]
fn invalid_nutrients_are_omitted_without_losing_valid_siblings() -> Result<(), Box<dyn Error>> {
    let parsed = super::parse_food_data(
        r#"{"foodNutrients":[
          {"nutrient":{"id":1003,"unitName":"kcal"},"amount":1},
          {"nutrient":{"id":1004,"unitName":"g"},"amount":{"invalid":true}},
          {"nutrient":{"id":1005,"unitName":"g"},"amount":3,"loq":"~ 0.5"},
          {"nutrient":{"id":1079,"unitName":"g"},"amount":4}
        ]}"#,
        FoodKind::Foundation,
    )?;
    let profile = parsed
        .food_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("valid sibling nutrients were lost"))?;
    assert_eq!(profile.nutrients.protein, None);
    assert_eq!(profile.nutrients.fat.total, None);
    assert_eq!(profile.nutrients.carbohydrates.including_fibre, None);
    assert_eq!(
        amount_text(&profile.nutrients.fibre),
        Some("4 g".to_owned())
    );
    assert_eq!(parsed.sanitizations.len(), 3);
    Ok(())
}

#[test]
fn malformed_top_level_record_json_still_fails() -> Result<(), Box<dyn Error>> {
    let error = parse_error("{")?;
    assert!(matches!(error, NutritionError::Record(_)));
    Ok(())
}

#[test]
fn ambiguous_duplicates_are_omitted_and_energy_and_sugar_fall_back() -> Result<(), Box<dyn Error>> {
    let parsed = super::parse_food_data(
        r#"{"foodNutrients":[
          {"nutrient":{"id":2048,"unitName":"kcal"},"amount":1},
          {"nutrient":{"id":2048,"unitName":"kcal"},"amount":2},
          {"nutrient":{"id":2048,"unitName":"kcal"},"amount":3},
          {"nutrient":{"id":2047,"unitName":"kcal"},"amount":20},
          {"nutrient":{"id":1063,"unitName":"g"},"amount":1},
          {"nutrient":{"id":1063,"unitName":"g"},"amount":2},
          {"nutrient":{"id":1063,"unitName":"g"},"amount":3},
          {"nutrient":{"id":2000,"unitName":"g"},"amount":5},
          {"nutrient":{"id":1003,"unitName":"g"},"amount":6},
          {"nutrient":{"id":1003,"unitName":"g"},"amount":7},
          {"nutrient":{"id":1003,"unitName":"g"},"amount":8}
        ]}"#,
        FoodKind::Foundation,
    )?;
    let profile = parsed
        .food_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("fallback nutrient values are missing"))?;
    assert_eq!(profile.nutrients.protein, None);
    assert_eq!(
        profile.energy.as_ref().map(ToString::to_string),
        Some("20 kcal".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.carbohydrates.sugars.total),
        Some("5 g".to_owned())
    );
    for field in ["nutrient[2048]", "nutrient[1063]", "nutrient[1003]"] {
        assert!(
            parsed
                .sanitizations
                .iter()
                .any(|entry| entry.field == field)
        );
    }
    Ok(())
}

#[test]
fn invalid_duplicate_nutrients_do_not_make_valid_values_ambiguous() -> Result<(), Box<dyn Error>> {
    let valid = r#"{"nutrient":{"id":1003,"unitName":"g"},"amount":6}"#;
    for (unit, amount, diagnostic_field) in [
        ("oz", "7", "nutrient[1003]"),
        ("g", "\"bad\"", "nutrient[1003]"),
        ("g", "-7", "protein"),
    ] {
        let invalid =
            format!(r#"{{"nutrient":{{"id":1003,"unitName":"{unit}"}},"amount":{amount}}}"#);
        for valid_first in [true, false] {
            for extra_valid in [false, true] {
                let nutrients = if valid_first {
                    format!("{valid},{invalid}")
                } else {
                    format!("{invalid},{valid}")
                };
                let nutrients = if extra_valid {
                    format!("{nutrients},{valid}")
                } else {
                    nutrients
                };
                let parsed = super::parse_food_data(
                    &format!(r#"{{"foodNutrients":[{nutrients}]}}"#),
                    FoodKind::Foundation,
                )?;
                let expected = if extra_valid { None } else { Some("6 g") };
                assert_eq!(
                    parsed
                        .food_data
                        .nutrition
                        .as_ref()
                        .and_then(|profile| profile.nutrients.protein.as_ref())
                        .map(ToString::to_string)
                        .as_deref(),
                    expected
                );
                let mut expected_fields = vec![diagnostic_field];
                if extra_valid {
                    expected_fields.push("nutrient[1003]");
                }
                let fields: Vec<_> = parsed
                    .sanitizations
                    .iter()
                    .map(|diagnostic| diagnostic.field.as_str())
                    .collect();
                assert_eq!(fields, expected_fields);
            }
        }
    }
    Ok(())
}

#[test]
fn malformed_optional_arrays_and_entries_do_not_lose_valid_siblings() -> Result<(), Box<dyn Error>>
{
    let parsed = super::parse_food_data(
        r#"{"foodNutrients":[
          {"nutrient":{"id":1003,"unitName":"g"},"amount":0.1000000000000000000000000001},
          {"nutrient":{"id":"bad","unitName":"g"},"amount":2},
          {"nutrient":{"id":1079,"unitName":"g"},"amount":0.5}
        ],"foodPortions":[
          {"portionDescription":"valid serving","gramWeight":30},
          {"portionDescription":"bad serving","gramWeight":-1},
          {"portionDescription":40,"gramWeight":50}
        ]}"#,
        FoodKind::Survey,
    )?;
    let profile = parsed
        .food_data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("valid nutrient entries were lost"))?;
    assert_eq!(
        amount_text(&profile.nutrients.protein),
        Some("0.1000000000000000000000000001 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.nutrients.fibre),
        Some("0.5 g".to_owned())
    );
    assert_eq!(parsed.food_data.serving_sizes.len(), 1);
    assert_eq!(
        parsed.food_data.serving_sizes[0].quantity().to_string(),
        "30 g"
    );
    assert_eq!(parsed.sanitizations.len(), 3);

    let malformed_arrays = super::parse_food_data(
        r#"{"foodNutrients":{"invalid":true},"foodPortions":{"invalid":true}}"#,
        FoodKind::Survey,
    )?;
    assert_eq!(malformed_arrays.food_data.nutrition, None);
    assert!(malformed_arrays.food_data.serving_sizes.is_empty());
    assert_eq!(malformed_arrays.sanitizations.len(), 2);
    Ok(())
}

#[test]
fn accepts_numeric_escaped_and_whitespace_source_scalars() -> Result<(), Box<dyn Error>> {
    let numeric = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":0.5}]}"#,
        FoodKind::Foundation,
    )?;
    let numeric_amount = numeric
        .nutrition
        .as_ref()
        .and_then(|profile| profile.nutrients.protein.as_ref())
        .map(ToString::to_string);
    assert_eq!(numeric_amount, Some("0.5 g".to_owned()));

    let escaped = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1093,"unitName":"m\u0067"},"amount":"  \u003c 0.500  "}]}"#,
        FoodKind::Foundation,
    )?;
    let escaped_amount = escaped
        .nutrition
        .as_ref()
        .and_then(|profile| profile.nutrients.sodium.as_ref())
        .map(ToString::to_string);
    assert_eq!(escaped_amount, Some("< 0.500 mg".to_owned()));

    let whitespace = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":"  ~ 0.500  "}]}"#,
        FoodKind::Foundation,
    )?;
    let whitespace_amount = whitespace
        .nutrition
        .as_ref()
        .and_then(|profile| profile.nutrients.protein.as_ref())
        .map(ToString::to_string);
    assert_eq!(whitespace_amount, Some("~ 0.500 g".to_owned()));

    let unsupported = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":999999,"unitName":{"ignored":true}},"amount":"invalid"}]}"#,
        FoodKind::Foundation,
    )?;
    assert_eq!(unsupported.nutrition, None);
    Ok(())
}

#[test]
fn maps_extended_nutrients_to_their_source_independent_fields() -> Result<(), Box<dyn Error>> {
    let nutrients = [
        (1051, "G", "water"),
        (1057, "MG", "caffeine"),
        (1180, "MG", "choline"),
        (1087, "MG", "minerals.calcium"),
        (1089, "MG", "minerals.iron"),
        (1090, "MG", "minerals.magnesium"),
        (1091, "MG", "minerals.phosphorus"),
        (1092, "MG", "minerals.potassium"),
        (1095, "MG", "minerals.zinc"),
        (1098, "MG", "minerals.copper"),
        (1101, "MG", "minerals.manganese"),
        (1103, "UG", "minerals.selenium"),
        (1106, "UG", "vitamins.vitamin_a"),
        (1162, "MG", "vitamins.vitamin_c"),
        (1114, "UG", "vitamins.vitamin_d"),
        (1109, "MG", "vitamins.vitamin_e"),
        (1185, "UG", "vitamins.vitamin_k1"),
        (1165, "MG", "vitamins.vitamin_b1"),
        (1166, "MG", "vitamins.vitamin_b2"),
        (1167, "MG", "vitamins.vitamin_b3"),
        (1170, "MG", "vitamins.vitamin_b5"),
        (1175, "MG", "vitamins.vitamin_b6"),
        (1177, "UG", "vitamins.vitamin_b9"),
        (1178, "UG", "vitamins.vitamin_b12"),
    ];
    for (id, unit, field) in nutrients {
        let input = format!(
            r#"{{"foodNutrients":[{{"nutrient":{{"id":{id},"unitName":"{unit}"}},"amount":1}}]}}"#
        );
        let data = parse_data(&input, FoodKind::Foundation)?;
        let profile = data
            .nutrition
            .ok_or_else(|| io::Error::other("nutrition profile is missing"))?;
        let value = serde_json::to_value(profile)?;
        let actual = field.split('.').fold(&value, |value, part| &value[part]);
        let expected_unit = match unit {
            "G" => "g",
            "MG" => "mg",
            _ => "µg",
        };
        assert_eq!(
            actual.as_str(),
            Some(format!("1 {expected_unit}").as_str()),
            "nutrient {id}"
        );
    }
    Ok(())
}

#[test]
fn extended_nutrients_keep_missing_zero_limits_and_invalid_values_distinct()
-> Result<(), Box<dyn Error>> {
    let parsed = super::parse_food_data(
        r#"{"foodNutrients":[
        {"nutrient":{"id":1051,"unitName":"g"},"amount":0},
        {"nutrient":{"id":1057,"unitName":"mg"},"amount":null},
        {"nutrient":{"id":1090,"unitName":"mg"},"amount":0,"loq":0.5},
        {"nutrient":{"id":1092,"unitName":"mg"},"amount":-1},
        {"nutrient":{"id":1106,"unitName":"ug"},"amount":"bad"},
        {"nutrient":{"id":1162,"unitName":"mg"},"amount":1},
        {"nutrient":{"id":1162,"unitName":"mg"},"amount":2}
    ]}"#,
        FoodKind::Foundation,
    )?;
    let profile = parsed
        .food_data
        .nutrition
        .ok_or_else(|| io::Error::other("nutrition profile is missing"))?;
    assert_eq!(
        amount_text(&profile.nutrients.water).as_deref(),
        Some("0 g")
    );
    assert_eq!(profile.nutrients.caffeine, None);
    assert_eq!(
        amount_text(&profile.nutrients.minerals.magnesium).as_deref(),
        Some("< 0.5 mg")
    );
    assert_eq!(profile.nutrients.minerals.potassium, None);
    assert_eq!(profile.nutrients.vitamins.vitamin_a, None);
    assert_eq!(profile.nutrients.vitamins.vitamin_c, None);
    for field in ["minerals.potassium", "nutrient[1106]", "nutrient[1162]"] {
        assert!(
            parsed
                .sanitizations
                .iter()
                .any(|entry| entry.field == field),
            "missing diagnostic for {field}"
        );
    }
    Ok(())
}

#[test]
fn does_not_conflate_other_vitamin_forms() -> Result<(), Box<dyn Error>> {
    for id in [1104, 1110, 1190, 2067, 2068] {
        let input = format!(
            r#"{{"foodNutrients":[{{"nutrient":{{"id":{id},"unitName":"ug"}},"amount":1}}]}}"#
        );
        let parsed = super::parse_food_data(&input, FoodKind::Foundation)?;
        assert_eq!(
            parsed.food_data.nutrition, None,
            "unexpected mapping for nutrient {id}"
        );
        assert!(parsed.sanitizations.is_empty());
    }
    Ok(())
}

fn parse_error(input: &str) -> Result<NutritionError, Box<dyn Error>> {
    parse_data(input, FoodKind::Foundation)
        .err()
        .ok_or_else(|| io::Error::other("input unexpectedly parsed successfully").into())
}

fn amount_text<T: Measurement>(amount: &Option<Reported<T>>) -> Option<String> {
    amount.as_ref().map(ToString::to_string)
}
