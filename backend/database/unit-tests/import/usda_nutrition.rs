use std::error::Error as StdError;
use std::io;

use crate::nutrition::{FoodData, NutritionBasis, NutritionProfile};

use super::*;

#[test]
fn maps_all_supported_nutrients_with_their_supplied_units() -> Result<(), Box<dyn StdError>> {
    let cases = [
        (1003, "G", "1.001", "1.001 g"),
        (1004, "MG", "2.002", "2.002 mg"),
        (1005, "UG", "3.003", "3.003 µg"),
        (1008, "KCAL", "4.004", "4.004 kcal"),
        (1009, "MG", "5.005", "5.005 mg"),
        (1013, "UG", "6.006", "6.006 µg"),
        (1062, "KJ", "7.007", "7.007 kJ"),
        (1063, "G", "8.008", "8.008 g"),
        (1079, "MG", "9.009", "9.009 mg"),
        (1093, "UG", "10.01", "10.01 µg"),
        (1235, "G", "11.011", "11.011 g"),
        (1253, "MG", "12.012", "12.012 mg"),
        (1257, "UG", "13.013", "13.013 µg"),
        (1258, "G", "14.014", "14.014 g"),
        (1292, "MG", "15.015", "15.015 mg"),
        (1293, "UG", "16.016", "16.016 µg"),
        (2000, "G", "17.017", "17.017 g"),
        (2047, "KCAL", "18.018", "18.018 kcal"),
        (2048, "KCAL", "19.019", "19.019 kcal"),
    ];

    for (id, unit, amount, expected) in cases {
        let data = parse_nutrients(&[nutrient(id, unit, amount)], FoodKind::Foundation)?;
        let profile = profile(&data)?;

        assert_eq!(profile.per, NutritionBasis::Per100Grams, "nutrient {id}");
        assert_eq!(
            nutrient_text(profile, id).as_deref(),
            Some(expected),
            "nutrient {id}"
        );
    }

    Ok(())
}

#[test]
fn energy_priority_is_stable_across_input_order() -> Result<(), Box<dyn StdError>> {
    let entries = [
        nutrient(1062, "KJ", "600"),
        nutrient(1008, "KCAL", "100"),
        nutrient(2047, "KCAL", "200"),
        nutrient(2048, "KCAL", "300"),
    ];
    let data = parse_nutrients(&entries, FoodKind::Foundation)?;

    assert_eq!(
        profile(&data)?
            .energy
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("300 kcal")
    );
    Ok(())
}

#[test]
fn energy_falls_back_when_higher_priority_ids_are_missing() -> Result<(), Box<dyn StdError>> {
    let cases: &[(&[u32], &str)] = &[
        (&[2047, 1008, 1062], "2047 kcal"),
        (&[1008, 1062], "1008 kcal"),
        (&[1062], "1062 kJ"),
    ];

    for (available, expected) in cases {
        let entries = available
            .iter()
            .map(|id| energy_nutrient(*id))
            .collect::<Vec<_>>();
        let data = parse_nutrients(&entries, FoodKind::Foundation)?;

        assert_eq!(
            profile(&data)?
                .energy
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some(*expected)
        );
    }

    Ok(())
}

#[test]
fn energy_falls_back_when_candidate_amounts_are_null() -> Result<(), Box<dyn StdError>> {
    for id in [2048, 2047, 1008, 1062] {
        let mut entries = [2048, 2047, 1008, 1062]
            .into_iter()
            .map(energy_nutrient)
            .collect::<Vec<_>>();
        let candidate = entries
            .iter_mut()
            .find(|entry| entry.contains(&format!("\"id\":{id}")))
            .ok_or_else(|| io::Error::other(format!("missing energy candidate {id}")))?;
        *candidate = nutrient_with_null_amount(id, energy_unit(id));
        let data = parse_nutrients(&entries, FoodKind::Foundation)?;
        let expected_id = if id == 2048 { 2047 } else { 2048 };
        let expected = energy_value(expected_id);

        assert_eq!(
            profile(&data)?
                .energy
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some(expected),
            "null amount for nutrient {id}"
        );
    }

    Ok(())
}

#[test]
fn zero_energy_wins_and_absent_energy_stays_none() -> Result<(), Box<dyn StdError>> {
    let entries = [nutrient(2048, "KCAL", "0"), nutrient(2047, "KCAL", "200")];
    let data = parse_nutrients(&entries, FoodKind::Foundation)?;
    assert_eq!(
        profile(&data)?
            .energy
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("0 kcal")
    );

    let data = parse_nutrients(&[nutrient(1003, "G", "1")], FoodKind::Foundation)?;
    assert_eq!(profile(&data)?.energy, None);
    Ok(())
}

#[test]
fn invalid_lower_priority_energy_candidates_still_fail() {
    let entries = [nutrient(2048, "KCAL", "200"), nutrient(1062, "KJ", "-1")];

    assert!(matches!(
        parse_nutrients(&entries, FoodKind::Foundation),
        Err(NutritionError::Nutrient { id: 1062, .. })
    ));

    let invalid_preferred = [nutrient(2048, "KCAL", "-1"), nutrient(2047, "KCAL", "200")];
    assert!(matches!(
        parse_nutrients(&invalid_preferred, FoodKind::Foundation),
        Err(NutritionError::Nutrient { id: 2048, .. })
    ));
}

#[test]
fn energy_ids_reject_values_with_the_wrong_energy_unit() {
    assert!(matches!(
        parse_nutrients(&[nutrient(2048, "KJ", "100")], FoodKind::Foundation),
        Err(NutritionError::EnergyUnit(2048))
    ));
}

#[test]
fn preserves_decimal_tokens_and_parses_scientific_notation() -> Result<(), Box<dyn StdError>> {
    let entries = [
        nutrient(1003, "G", "0.10000000000000001"),
        nutrient(1079, "MG", "1.2345e-2"),
    ];
    let data = parse_nutrients(&entries, FoodKind::Foundation)?;
    let profile = profile(&data)?;

    assert_eq!(
        profile.protein.as_ref().map(ToString::to_string).as_deref(),
        Some("0.10000000000000001 g")
    );
    assert_eq!(
        profile.fibre.as_ref().map(ToString::to_string).as_deref(),
        Some("0.012345 mg")
    );
    Ok(())
}

#[test]
fn retains_reported_amount_qualifiers() -> Result<(), Box<dyn StdError>> {
    let cases = [
        ("< 0.3", "< 0.3 g"),
        ("<= 0.3", "<= 0.3 g"),
        ("> 0.3", "> 0.3 g"),
        (">= 0.3", ">= 0.3 g"),
        ("~ 0.3", "~ 0.3 g"),
    ];

    for (amount, expected) in cases {
        let quoted_amount = format!(r#""{amount}""#);
        let data = parse_nutrients(&[nutrient(1003, "G", &quoted_amount)], FoodKind::Foundation)?;

        assert_eq!(
            profile(&data)?
                .protein
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some(expected)
        );
    }

    Ok(())
}

#[test]
fn converts_positive_loq_with_zero_amount_to_a_below_limit_report() -> Result<(), Box<dyn StdError>>
{
    let entry = nutrient_with_loq(1003, "G", "0", "0.03");
    let data = parse_nutrients(&[entry], FoodKind::Foundation)?;

    assert_eq!(
        profile(&data)?
            .protein
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("< 0.03 g")
    );
    Ok(())
}

#[test]
fn rejects_negative_and_inconsistent_loq_values() {
    let negative = nutrient_with_loq(1003, "G", "0", "-0.03");
    assert!(matches!(
        parse_nutrients(&[negative], FoodKind::Foundation),
        Err(NutritionError::InvalidLimit(1003))
    ));

    let inconsistent = nutrient_with_loq(1003, "G", "0.01", "0.03");
    assert!(matches!(
        parse_nutrients(&[inconsistent], FoodKind::Foundation),
        Err(NutritionError::InvalidLimit(1003))
    ));
}

#[test]
fn absent_amount_remains_none() -> Result<(), Box<dyn StdError>> {
    let entries = [nutrient_without_amount(1003, "G"), nutrient(1004, "G", "2")];
    let data = parse_nutrients(&entries, FoodKind::Foundation)?;
    let profile = profile(&data)?;

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
    Ok(())
}

#[test]
fn preserves_carbohydrate_and_sugar_definitions_without_inference() -> Result<(), Box<dyn StdError>>
{
    let entries = [
        nutrient(2000, "G", "3"),
        nutrient(1235, "G", "0.5"),
        nutrient(1013, "G", "1"),
        nutrient(1005, "G", "12"),
        nutrient(1063, "G", "2"),
        nutrient(1093, "MG", "250"),
    ];
    let data = parse_nutrients(&entries, FoodKind::Foundation)?;
    let profile = profile(&data)?;

    assert_eq!(
        profile
            .carbohydrates
            .including_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("12 g")
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
        Some("3 g")
    );
    assert_eq!(
        profile
            .carbohydrates
            .sugars
            .lactose
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("1 g")
    );
    assert_eq!(
        profile
            .carbohydrates
            .sugars
            .added
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("0.5 g")
    );
    assert_eq!(
        profile.sodium.as_ref().map(ToString::to_string).as_deref(),
        Some("250 mg")
    );
    assert_eq!(profile.salt, None);
    Ok(())
}

#[test]
fn unsupported_nutrients_do_not_create_a_nutrition_profile() -> Result<(), Box<dyn StdError>> {
    let data = parse_with_fields(
        r#""foodNutrients":[{"nutrient":{"id":999999,"unitName":"unknown"},"amount":"not a number"}]"#,
        FoodKind::Foundation,
    )?;

    assert_eq!(data.nutrition, None);
    Ok(())
}

#[test]
fn branded_nutrients_use_the_declared_millilitre_basis_and_standard_values()
-> Result<(), Box<dyn StdError>> {
    let data = parse_with_fields(
        r#""servingSize":250,"servingSizeUnit":"ml","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":4},{"nutrient":{"id":1008,"unitName":"kcal"},"amount":50}]"#,
        FoodKind::Branded,
    )?;
    let profile = profile(&data)?;

    assert_eq!(profile.per, NutritionBasis::Per100Millilitres);
    assert_eq!(
        profile.protein.as_ref().map(ToString::to_string).as_deref(),
        Some("4 g")
    );
    assert_eq!(
        profile.energy.as_ref().map(ToString::to_string).as_deref(),
        Some("50 kcal")
    );
    Ok(())
}

#[test]
fn label_nutrients_alone_do_not_create_standardized_nutrition() -> Result<(), Box<dyn StdError>> {
    let data = parse_with_fields(
        r#""labelNutrients":{"calories":{"value":120},"protein":{"value":5}}"#,
        FoodKind::Branded,
    )?;

    assert_eq!(data.nutrition, None);
    Ok(())
}

#[test]
fn branded_nutrients_without_a_known_mass_or_volume_basis_fail() {
    let record = r#"{"servingSize":28,"servingSizeUnit":"oz","foodNutrients":[{"nutrient":{"id":1003,"unitName":"g"},"amount":4}]}"#;

    assert!(matches!(
        parse(record, FoodKind::Branded),
        Err(NutritionError::UnknownBasis)
    ));
}

#[test]
fn measured_portions_keep_names_and_weights_and_skip_missing_weights()
-> Result<(), Box<dyn StdError>> {
    let data = parse_with_fields(
        r#""foodPortions":[{"portionDescription":"slice","gramWeight":20},{"amount":2,"modifier":"medium","measureUnit":{"name":"piece"},"gramWeight":45},{"portionDescription":"unweighted portion","gramWeight":null},{"amount":1,"measureUnit":{"name":"cup"}}]"#,
        FoodKind::Foundation,
    )?;

    assert_eq!(data.serving_sizes.len(), 2);
    let first = data
        .serving_sizes
        .first()
        .ok_or_else(|| io::Error::other("missing first measured portion"))?;
    let second = data
        .serving_sizes
        .get(1)
        .ok_or_else(|| io::Error::other("missing second measured portion"))?;

    assert_eq!(first.name, "slice");
    assert_eq!(first.quantity.to_string(), "20 g");
    assert_eq!(second.name, "2 piece, medium");
    assert_eq!(second.quantity.to_string(), "45 g");
    Ok(())
}

fn parse_nutrients(nutrients: &[String], kind: FoodKind) -> Result<FoodData, NutritionError> {
    let nutrients = nutrients.join(",");
    parse_with_fields(&format!(r#""foodNutrients":[{nutrients}]"#), kind)
}

fn parse_with_fields(fields: &str, kind: FoodKind) -> Result<FoodData, NutritionError> {
    parse(&format!("{{{fields}}}"), kind)
}

fn nutrient(id: u32, unit: &str, amount_json: &str) -> String {
    format!(r#"{{"nutrient":{{"id":{id},"unitName":"{unit}"}},"amount":{amount_json}}}"#)
}

fn nutrient_without_amount(id: u32, unit: &str) -> String {
    format!(r#"{{"nutrient":{{"id":{id},"unitName":"{unit}"}}}}"#)
}

fn nutrient_with_null_amount(id: u32, unit: &str) -> String {
    format!(r#"{{"nutrient":{{"id":{id},"unitName":"{unit}"}},"amount":null}}"#)
}

fn nutrient_with_loq(id: u32, unit: &str, amount_json: &str, loq_json: &str) -> String {
    format!(
        r#"{{"nutrient":{{"id":{id},"unitName":"{unit}"}},"amount":{amount_json},"loq":{loq_json}}}"#
    )
}

fn energy_nutrient(id: u32) -> String {
    let amount = id.to_string();
    nutrient(id, energy_unit(id), &amount)
}

fn energy_unit(id: u32) -> &'static str {
    match id {
        1062 => "KJ",
        _ => "KCAL",
    }
}

fn energy_value(id: u32) -> &'static str {
    match id {
        2048 => "2048 kcal",
        2047 => "2047 kcal",
        1008 => "1008 kcal",
        1062 => "1062 kJ",
        _ => "",
    }
}

fn nutrient_text(profile: &NutritionProfile, id: u32) -> Option<String> {
    match id {
        1003 => profile.protein.as_ref().map(ToString::to_string),
        1004 => profile.fat.total.as_ref().map(ToString::to_string),
        1005 => profile
            .carbohydrates
            .including_fibre
            .as_ref()
            .map(ToString::to_string),
        1008 | 1062 | 2047 | 2048 => profile.energy.as_ref().map(ToString::to_string),
        1009 => profile
            .carbohydrates
            .starch
            .as_ref()
            .map(ToString::to_string),
        1013 => profile
            .carbohydrates
            .sugars
            .lactose
            .as_ref()
            .map(ToString::to_string),
        1063 | 2000 => profile
            .carbohydrates
            .sugars
            .total
            .as_ref()
            .map(ToString::to_string),
        1079 => profile.fibre.as_ref().map(ToString::to_string),
        1093 => profile.sodium.as_ref().map(ToString::to_string),
        1235 => profile
            .carbohydrates
            .sugars
            .added
            .as_ref()
            .map(ToString::to_string),
        1253 => profile.cholesterol.as_ref().map(ToString::to_string),
        1257 => profile.fat.trans.as_ref().map(ToString::to_string),
        1258 => profile.fat.saturated.as_ref().map(ToString::to_string),
        1292 => profile
            .fat
            .monounsaturated
            .as_ref()
            .map(ToString::to_string),
        1293 => profile
            .fat
            .polyunsaturated
            .as_ref()
            .map(ToString::to_string),
        _ => None,
    }
}

fn profile(data: &FoodData) -> Result<&NutritionProfile, io::Error> {
    data.nutrition
        .as_ref()
        .ok_or_else(|| io::Error::other("expected a nutrition profile"))
}
