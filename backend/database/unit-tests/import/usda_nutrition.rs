use std::error::Error;
use std::io;

use super::*;
use crate::nutrition::{NutritionBasis, Qualifier, QuantityError, Reported};

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
    let data = parse_food_data(input, FoodKind::Foundation)?;
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
        let data = parse_food_data(&input, kind)?;
        assert_eq!(
            data.nutrition.as_ref().map(|profile| profile.per),
            Some(NutritionBasis::Per100Grams)
        );
    }

    let mass = parse_food_data(
        r#"{"servingSize":30,"servingSizeUnit":"G","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1}]}"#,
        FoodKind::Branded,
    )?;
    assert_eq!(
        mass.nutrition.as_ref().map(|profile| profile.per),
        Some(NutritionBasis::Per100Grams)
    );

    let volume = parse_food_data(
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
        parse_food_data(
            r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1}]}"#,
            FoodKind::Branded,
        )
        .is_err()
    );
    assert!(
        parse_food_data(
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
        let data = parse_food_data(input, FoodKind::Foundation)?;
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
    let data = parse_food_data(input, FoodKind::Foundation)?;
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
        let data = parse_food_data(&input, FoodKind::Foundation)?;
        let actual = data
            .nutrition
            .as_ref()
            .and_then(|profile| profile.energy.as_ref().map(ToString::to_string));
        assert_eq!(actual, Some(expected.to_owned()));
    }

    let no_energy = parse_food_data(
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
    assert!(parse_food_data(input, FoodKind::Foundation).is_err());
    Ok(())
}

#[test]
fn rejects_negative_fdc_nutrient_and_energy_values() -> Result<(), Box<dyn Error>> {
    let negative_carbohydrate =
        r#"{"foodNutrients":[{"nutrient":{"id":1005,"unitName":"g"},"amount":-0.475}]}"#;
    assert!(matches!(
        parse_food_data(negative_carbohydrate, FoodKind::Foundation),
        Err(NutritionError::Amount {
            source: QuantityError::Negative,
            ..
        })
    ));

    let negative_preferred_energy = r#"{"foodNutrients":[
      {"nutrient":{"id":2048,"unitName":"kcal"},"amount":-1},
      {"nutrient":{"id":2047,"unitName":"kcal"},"amount":20}
    ]}"#;
    assert!(matches!(
        parse_food_data(negative_preferred_energy, FoodKind::Foundation),
        Err(NutritionError::Amount {
            source: QuantityError::Negative,
            ..
        })
    ));
    Ok(())
}

#[test]
fn preserves_exact_decimal_lexemes_and_source_qualifiers() -> Result<(), Box<dyn Error>> {
    let input = r#"{"foodNutrients":[
      {"nutrient":{"id":1003,"unitName":"g"},"amount":0.1000000000000000000000000001},
      {"nutrient":{"id":1093,"unitName":"mg"},"amount":"< 0.5"}
    ]}"#;
    let data = parse_food_data(input, FoodKind::Foundation)?;
    let profile = data
        .nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("nutrition profile is missing"))?;
    assert_eq!(
        amount_text(&profile.protein),
        Some("0.1000000000000000000000000001 g".to_owned())
    );
    assert_eq!(
        profile.sodium.as_ref().map(|value| value.qualifier),
        Some(Qualifier::LessThan)
    );
    assert_eq!(amount_text(&profile.sodium), Some("< 0.5 mg".to_owned()));

    let loq = parse_food_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1079,"unitName":"µg"},"amount":0,"loq":0.55}]}"#,
        FoodKind::Foundation,
    )?;
    let fibre = loq
        .nutrition
        .as_ref()
        .and_then(|profile| profile.fibre.as_ref())
        .ok_or_else(|| io::Error::other("LOQ fibre value is missing"))?;
    assert_eq!(fibre.qualifier, Qualifier::LessThan);
    assert_eq!(fibre.to_string(), "< 0.55 µg");

    let measured = parse_food_data(
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
    let missing = parse_food_data(
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":null},{"nutrient":{"id":1013,"unitName":"g"},"amount":null}]}"#,
        FoodKind::Foundation,
    )?;
    assert_eq!(missing.nutrition, None);

    let partial = parse_food_data(
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
    let data = parse_food_data(input, FoodKind::Foundation)?;
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

    let no_volume = parse_food_data(
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
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":-1}]}"#,
        r#"{"foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":1},{"nutrient":{"id":1003,"unitName":"g"},"amount":2}]}"#,
    ] {
        assert!(parse_food_data(input, FoodKind::Foundation).is_err());
    }
    Ok(())
}

fn amount_text<T: std::fmt::Display>(amount: &Option<Reported<T>>) -> Option<String> {
    amount.as_ref().map(ToString::to_string)
}
