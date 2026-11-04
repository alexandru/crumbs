use std::error::Error;
use std::io;

use super::*;
use crumbs_database::nutrition::{NutritionBasis, Qualifier, QuantityError, Reported};

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
    assert_eq!(amount_text(&profile.protein), Some("7 g".to_owned()));
    assert_eq!(amount_text(&profile.fat.total), Some("8 g".to_owned()));
    assert_eq!(
        amount_text(&profile.carbohydrates.including_fibre),
        Some("9 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.carbohydrates.starch),
        Some("10 mg".to_owned())
    );
    assert_eq!(
        amount_text(&profile.carbohydrates.sugars.lactose),
        Some("11 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.carbohydrates.sugars.total),
        Some("12 mg".to_owned())
    );
    assert_eq!(amount_text(&profile.fibre), Some("14 g".to_owned()));
    assert_eq!(amount_text(&profile.sodium), Some("15 µg".to_owned()));
    assert_eq!(
        amount_text(&profile.carbohydrates.sugars.added),
        Some("16 g".to_owned())
    );
    assert_eq!(amount_text(&profile.cholesterol), Some("17 mg".to_owned()));
    assert_eq!(amount_text(&profile.fat.trans), Some("18 g".to_owned()));
    assert_eq!(amount_text(&profile.fat.saturated), Some("19 g".to_owned()));
    assert_eq!(
        amount_text(&profile.fat.monounsaturated),
        Some("20 g".to_owned())
    );
    assert_eq!(
        amount_text(&profile.fat.polyunsaturated),
        Some("21 g".to_owned())
    );
    assert_eq!(profile.carbohydrates.excluding_fibre, None);
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
    assert_eq!(serving.quantity.to_string(), "250 ml");
    assert!(
        parse_data(
            r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1}]}"#,
            FoodKind::Branded,
        )
        .is_err()
    );
    assert!(
        parse_data(
            r#"{"servingSize":0,"servingSizeUnit":"g"}"#,
            FoodKind::Branded,
        )
        .is_err()
    );
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
fn invalid_preferred_energy_is_not_hidden_by_lower_priority_value() -> Result<(), Box<dyn Error>> {
    let input = r#"{"foodNutrients":[
      {"nutrient":{"id":2048,"unitName":"g"},"amount":10},
      {"nutrient":{"id":2047,"unitName":"kcal"},"amount":20}
    ]}"#;
    assert!(parse_data(input, FoodKind::Foundation).is_err());
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
    assert_eq!(profile.carbohydrates.including_fibre, None);
    assert_eq!(profile.protein, None);
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
    assert_eq!(amount_text(&profile.fibre), Some("0.2 g".to_owned()));
    assert_eq!(profile.protein, None);
    assert_eq!(amount_text(&profile.fat.total), Some("3 mg".to_owned()));
    assert_eq!(profile.carbohydrates.including_fibre, None);
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
        amount_text(&profile.protein),
        Some("0.1000000000000000000000000001 g".to_owned())
    );
    assert_eq!(
        profile.sodium.as_ref().map(Reported::qualifier),
        Some(Qualifier::LessThan)
    );
    assert_eq!(amount_text(&profile.sodium), Some("< 0.5 mg".to_owned()));

    let loq = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1079,"unitName":"µg"},"amount":0,"loq":0.55}]}"#,
        FoodKind::Foundation,
    )?;
    let fibre = loq
        .nutrition
        .as_ref()
        .and_then(|profile| profile.fibre.as_ref())
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
            .and_then(|profile| profile.fibre.as_ref())
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
    assert_eq!(amount_text(&profile.protein), Some("0 g".to_owned()));
    assert_eq!(profile.carbohydrates.sugars.lactose, None);
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
            .map(|serving| serving.quantity.to_string()),
        Some("30 g".to_owned())
    );
    assert_eq!(
        data.serving_sizes
            .get(1)
            .map(|serving| serving.quantity.to_string()),
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
fn rejects_invalid_units_and_duplicate_same_id_values() -> Result<(), Box<dyn Error>> {
    for input in [
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"kcal"},"amount":1}]}"#,
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1},{"nutrient":{"id":1003,"unitName":"g"},"amount":2}]}"#,
    ] {
        assert!(parse_data(input, FoodKind::Foundation).is_err());
    }
    Ok(())
}

#[test]
fn preserves_measurement_error_order_and_text() -> Result<(), Box<dyn Error>> {
    let invalid_amount_and_limit = parse_error(
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":"bad","loq":"also bad"}]}"#,
    )?;
    assert!(matches!(
        &invalid_amount_and_limit,
        NutritionError::Amount {
            value,
            source: QuantityError::Decimal(source),
            ..
        } if value == "bad g" && source == "bad"
    ));
    assert_eq!(
        invalid_amount_and_limit.to_string(),
        "nutrient amount \"bad g\" is invalid: invalid decimal or unsupported precision/range: bad"
    );

    let invalid_unit_and_amount = parse_error(
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"stone"},"amount":"bad"}]}"#,
    )?;
    assert!(matches!(
        &invalid_unit_and_amount,
        NutritionError::IncompatibleUnit { nutrient_id: 1003, unit } if unit == "stone"
    ));
    assert_eq!(
        invalid_unit_and_amount.to_string(),
        "nutrient 1003 has incompatible unit \"stone\""
    );

    let invalid_limit_with_positive_amount = parse_error(
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1.5,"loq":"bad"}]}"#,
    )?;
    assert!(matches!(
        &invalid_limit_with_positive_amount,
        NutritionError::Amount {
            value,
            source: QuantityError::Decimal(source),
            ..
        } if value == "bad g" && source == "bad"
    ));
    assert_eq!(
        invalid_limit_with_positive_amount.to_string(),
        "nutrient amount \"bad g\" is invalid: invalid decimal or unsupported precision/range: bad"
    );

    let qualified_limit_with_positive_amount = parse_error(
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1.5,"loq":"~ 0.5"}]}"#,
    )?;
    assert!(matches!(
        qualified_limit_with_positive_amount,
        NutritionError::QualifiedLimitOfQuantification
    ));
    assert_eq!(
        "limit of quantification must be an exact measured amount",
        NutritionError::QualifiedLimitOfQuantification.to_string()
    );
    Ok(())
}

#[test]
fn defers_energy_duplicates_but_rejects_sugar_duplicates_during_parsing()
-> Result<(), Box<dyn Error>> {
    let duplicate_energy_then_invalid_mass = parse_error(
        r#"{"foodNutrients":[
          {"nutrient":{"id":2048,"unitName":"kcal"},"amount":1},
          {"nutrient":{"id":2048,"unitName":"kcal"},"amount":2},
          {"nutrient":{"id":1003,"unitName":"g"},"amount":"bad"}
        ]}"#,
    )?;
    assert!(matches!(
        duplicate_energy_then_invalid_mass,
        NutritionError::Amount { .. }
    ));

    let duplicate_energy = parse_error(
        r#"{"foodNutrients":[
          {"nutrient":{"id":2048,"unitName":"kcal"},"amount":1},
          {"nutrient":{"id":2048,"unitName":"kcal"},"amount":2},
          {"nutrient":{"id":1003,"unitName":"g"},"amount":3}
        ]}"#,
    )?;
    assert!(matches!(
        duplicate_energy,
        NutritionError::DuplicateNutrient { nutrient_id: 2048 }
    ));
    assert_eq!(
        "multiple source values map to nutrient 2048",
        NutritionError::DuplicateNutrient { nutrient_id: 2048 }.to_string()
    );

    let duplicate_sugar_before_invalid_mass = parse_error(
        r#"{"foodNutrients":[
          {"nutrient":{"id":1063,"unitName":"g"},"amount":1},
          {"nutrient":{"id":1063,"unitName":"g"},"amount":2},
          {"nutrient":{"id":1003,"unitName":"g"},"amount":"bad"}
        ]}"#,
    )?;
    assert!(matches!(
        duplicate_sugar_before_invalid_mass,
        NutritionError::DuplicateNutrient { nutrient_id: 1063 }
    ));

    let mixed_duplicates = parse_error(
        r#"{"foodNutrients":[
          {"nutrient":{"id":2048,"unitName":"kcal"},"amount":1},
          {"nutrient":{"id":2048,"unitName":"kcal"},"amount":2},
          {"nutrient":{"id":1063,"unitName":"g"},"amount":1},
          {"nutrient":{"id":1063,"unitName":"g"},"amount":2}
        ]}"#,
    )?;
    assert!(matches!(
        mixed_duplicates,
        NutritionError::DuplicateNutrient { nutrient_id: 1063 }
    ));
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
        .and_then(|profile| profile.protein.as_ref())
        .map(ToString::to_string);
    assert_eq!(numeric_amount, Some("0.5 g".to_owned()));

    let escaped = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1093,"unitName":"m\u0067"},"amount":"  \u003c 0.500  "}]}"#,
        FoodKind::Foundation,
    )?;
    let escaped_amount = escaped
        .nutrition
        .as_ref()
        .and_then(|profile| profile.sodium.as_ref())
        .map(ToString::to_string);
    assert_eq!(escaped_amount, Some("< 0.500 mg".to_owned()));

    let whitespace = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":"  ~ 0.500  "}]}"#,
        FoodKind::Foundation,
    )?;
    let whitespace_amount = whitespace
        .nutrition
        .as_ref()
        .and_then(|profile| profile.protein.as_ref())
        .map(ToString::to_string);
    assert_eq!(whitespace_amount, Some("~ 0.500 g".to_owned()));

    let unsupported = parse_data(
        r#"{"foodNutrients":[{"nutrient":{"id":999999,"unitName":{"ignored":true}},"amount":"invalid"}]}"#,
        FoodKind::Foundation,
    )?;
    assert_eq!(unsupported.nutrition, None);
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
