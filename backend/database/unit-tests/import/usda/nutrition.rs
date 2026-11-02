use super::*;

#[test]
fn maps_supported_nutrient_ids_and_ignores_unmodeled_data() -> Result<(), Box<dyn std::error::Error>>
{
    let input = food(&[
        nutrient(1003, "g", "2"),
        nutrient(1004, "g", "3"),
        nutrient(1005, "g", "4"),
        nutrient(2048, "kcal", "10"),
        nutrient(2047, "kcal", "11"),
        nutrient(1008, "kcal", "12"),
        nutrient(1062, "kJ", "44"),
        nutrient(1009, "g", "1.1"),
        nutrient(1013, "g", "0.3"),
        nutrient(1079, "g", "0.5"),
        nutrient(1093, "mg", "250"),
        nutrient(1253, "mg", "5"),
        nutrient(1257, "g", "0.1"),
        nutrient(1258, "g", "1.4"),
        nutrient(1292, "g", "0.7"),
        nutrient(1293, "g", "0.8"),
        nutrient(1235, "g", "0.2"),
        nutrient(2000, "g", "1.8"),
        nutrient(9000, "odd", "99"),
        nutrient(9001, "odd", "{\"ignored\":true}"),
        r#"{"amount":3,"nutrient":{"unitName":"g"}}"#.to_owned(),
    ]);

    let parsed = parse(&input, FoodKind::Foundation)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or("supported nutrient values were not parsed")?;

    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(display(profile.protein.as_ref()), Some("2 g".to_owned()));
    assert_eq!(display(profile.fat.total.as_ref()), Some("3 g".to_owned()));
    assert_eq!(
        display(profile.fat.saturated.as_ref()),
        Some("1.4 g".to_owned())
    );
    assert_eq!(
        display(profile.fat.monounsaturated.as_ref()),
        Some("0.7 g".to_owned())
    );
    assert_eq!(
        display(profile.fat.polyunsaturated.as_ref()),
        Some("0.8 g".to_owned())
    );
    assert_eq!(
        display(profile.fat.trans.as_ref()),
        Some("0.1 g".to_owned())
    );
    assert_eq!(
        display(profile.carbohydrates.including_fibre.as_ref()),
        Some("4 g".to_owned())
    );
    assert_eq!(profile.carbohydrates.excluding_fibre, None);
    assert_eq!(
        display(profile.carbohydrates.starch.as_ref()),
        Some("1.1 g".to_owned())
    );
    assert_eq!(
        display(profile.carbohydrates.sugars.total.as_ref()),
        Some("1.8 g".to_owned())
    );
    assert_eq!(
        display(profile.carbohydrates.sugars.added.as_ref()),
        Some("0.2 g".to_owned())
    );
    assert_eq!(
        display(profile.carbohydrates.sugars.lactose.as_ref()),
        Some("0.3 g".to_owned())
    );
    assert_eq!(display(profile.fibre.as_ref()), Some("0.5 g".to_owned()));
    assert_eq!(display(profile.sodium.as_ref()), Some("250 mg".to_owned()));
    assert_eq!(
        display(profile.cholesterol.as_ref()),
        Some("5 mg".to_owned())
    );
    assert_eq!(display(profile.energy.as_ref()), Some("10 kcal".to_owned()));
    assert_eq!(profile.salt, None);
    Ok(())
}

#[test]
fn preserves_exact_decimal_spelling_and_string_qualifiers() -> Result<(), Box<dyn std::error::Error>>
{
    let input = food(&[
        nutrient(1003, "G", "0.100000000000000001"),
        nutrient(1258, "G", r#""< 1""#),
    ]);
    let parsed = parse(&input, FoodKind::SrLegacy)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or("supported nutrient values were not parsed")?;

    assert_eq!(
        display(profile.protein.as_ref()),
        Some("0.100000000000000001 g".to_owned())
    );
    assert_eq!(
        display(profile.fat.saturated.as_ref()),
        Some("< 1 g".to_owned())
    );
    assert_eq!(
        profile.fat.saturated.as_ref().map(Reported::qualifier),
        Some(crate::nutrition::Qualifier::LessThan)
    );
    Ok(())
}

#[test]
fn chooses_the_first_energy_candidate_with_an_amount() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            vec![nutrient(2048, "kcal", "10"), nutrient(2047, "kcal", "20")],
            "10 kcal",
        ),
        (
            vec![nutrient(2047, "kcal", "20"), nutrient(1008, "kcal", "30")],
            "20 kcal",
        ),
        (
            vec![nutrient(1008, "kcal", "30"), nutrient(1062, "kJ", "40")],
            "30 kcal",
        ),
        (vec![nutrient(1062, "kJ", "40")], "40 kJ"),
    ];

    for (nutrients, expected) in cases {
        let parsed = parse(&food(&nutrients), FoodKind::Survey)?;
        let profile = parsed
            .nutrition
            .as_ref()
            .ok_or("energy profile was not parsed")?;
        assert_eq!(display(profile.energy.as_ref()), Some(expected.to_owned()));
    }
    Ok(())
}

#[test]
fn missing_and_ignored_lower_energy_amounts_do_not_block_the_winner()
-> Result<(), Box<dyn std::error::Error>> {
    let input = r#"{
        "foodNutrients":[
          {"nutrient":{"id":2048,"unitName":"kcal"},"amount":null},
          {"nutrient":{"id":2047,"unitName":"kcal"},"amount":2},
          {"nutrient":{"id":1008,"unitName":"kcal"},"amount":"not a number"}
        ]
    }"#;
    let parsed = parse(input, FoodKind::Foundation)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or("energy profile was not parsed")?;
    assert_eq!(display(profile.energy.as_ref()), Some("2 kcal".to_owned()));
    Ok(())
}

#[test]
fn invalid_energy_winners_and_wrong_nutrient_dimensions_are_typed_errors() {
    let invalid_winner = food(&[
        nutrient(2048, "kcal", r#""not a number""#),
        nutrient(2047, "kcal", "2"),
    ]);
    assert!(matches!(
        parse(&invalid_winner, FoodKind::Foundation),
        Err(NutritionError::Nutrient {
            id: 2048,
            source: MeasurementError::InvalidDecimal,
        })
    ));

    let wrong_energy_unit = food(&[nutrient(2048, "kJ", "2")]);
    assert!(matches!(
        parse(&wrong_energy_unit, FoodKind::Foundation),
        Err(NutritionError::Nutrient {
            id: 2048,
            source: MeasurementError::InvalidUnit,
        })
    ));

    let wrong_mass_dimension = food(&[nutrient(1003, "kcal", "2")]);
    assert!(matches!(
        parse(&wrong_mass_dimension, FoodKind::Foundation),
        Err(NutritionError::Nutrient {
            id: 1003,
            source: MeasurementError::IncompatibleDimension,
        })
    ));
}

#[test]
fn duplicate_supported_amounts_are_rejected() {
    let input = food(&[nutrient(1003, "g", "1"), nutrient(1003, "g", "2")]);
    assert!(matches!(
        parse(&input, FoodKind::Foundation),
        Err(NutritionError::DuplicateNutrient { id: 1003 })
    ));
}

#[test]
fn total_sugars_accepts_alternate_id_and_keeps_primary_id_preferred()
-> Result<(), Box<dyn std::error::Error>> {
    let alternate = parse(&food(&[nutrient(1063, "g", "0.340")]), FoodKind::Foundation)?;
    let alternate_profile = alternate
        .nutrition
        .as_ref()
        .ok_or("alternate total sugars did not produce a profile")?;
    assert_eq!(
        display(alternate_profile.carbohydrates.sugars.total.as_ref()),
        Some("0.34 g".to_owned())
    );

    let primary = parse(
        &food(&[nutrient(2000, "g", "10.7"), nutrient(1063, "g", "10.6")]),
        FoodKind::Foundation,
    )?;
    let primary_profile = primary
        .nutrition
        .as_ref()
        .ok_or("primary total sugars did not produce a profile")?;
    assert_eq!(
        display(primary_profile.carbohydrates.sugars.total.as_ref()),
        Some("10.7 g".to_owned())
    );
    Ok(())
}

#[test]
fn missing_or_null_primary_total_sugars_falls_back_to_alternate()
-> Result<(), Box<dyn std::error::Error>> {
    for primary in [
        r#"{"nutrient":{"id":2000,"unitName":"g"}}"#,
        r#"{"nutrient":{"id":2000,"unitName":"g"},"amount":null}"#,
    ] {
        let input = format!(
            r#"{{"foodNutrients":[{primary},{{"nutrient":{{"id":1063,"unitName":"g"}},"amount":0.340}}]}}"#
        );
        let parsed = parse(&input, FoodKind::Foundation)?;
        let profile = parsed
            .nutrition
            .as_ref()
            .ok_or("alternate total sugars did not produce a profile")?;
        assert_eq!(
            display(profile.carbohydrates.sugars.total.as_ref()),
            Some("0.34 g".to_owned())
        );
    }
    Ok(())
}

#[test]
fn zero_primary_total_sugars_does_not_fall_back_to_alternate()
-> Result<(), Box<dyn std::error::Error>> {
    let input = food(&[nutrient(2000, "g", "0"), nutrient(1063, "g", "5")]);
    let parsed = parse(&input, FoodKind::Foundation)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or("zero total sugars did not produce a profile")?;
    assert_eq!(
        display(profile.carbohydrates.sugars.total.as_ref()),
        Some("0 g".to_owned())
    );
    Ok(())
}

#[test]
fn invalid_primary_total_sugars_does_not_use_alternate() {
    let input = food(&[
        nutrient(2000, "g", r#""not a number""#),
        nutrient(1063, "g", "5"),
    ]);
    assert!(matches!(
        parse(&input, FoodKind::Foundation),
        Err(NutritionError::Nutrient {
            id: 2000,
            source: MeasurementError::InvalidDecimal,
        })
    ));
}

#[test]
fn duplicate_selected_total_sugar_amounts_are_rejected() {
    let duplicate_primary = food(&[nutrient(2000, "g", "1"), nutrient(2000, "g", "2")]);
    assert!(matches!(
        parse(&duplicate_primary, FoodKind::Foundation),
        Err(NutritionError::DuplicateNutrient { id: 2000 })
    ));

    let duplicate_alternate = food(&[nutrient(1063, "g", "1"), nutrient(1063, "g", "2")]);
    assert!(matches!(
        parse(&duplicate_alternate, FoodKind::Foundation),
        Err(NutritionError::DuplicateNutrient { id: 1063 })
    ));
}

#[test]
fn selected_total_sugars_preserves_source_unit_qualifier_and_loq()
-> Result<(), Box<dyn std::error::Error>> {
    let qualified = food(&[nutrient(2000, "mg", r#""< 0.4""#)]);
    let parsed = parse(&qualified, FoodKind::Foundation)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or("qualified total sugars did not produce a profile")?;
    let reported = profile.carbohydrates.sugars.total.as_ref();
    assert_eq!(display(reported), Some("< 0.4 mg".to_owned()));
    assert_eq!(
        reported.map(Reported::qualifier),
        Some(crate::nutrition::Qualifier::LessThan)
    );

    let below_limit =
        r#"{"foodNutrients":[{"nutrient":{"id":1063,"unitName":"mg"},"amount":0,"loq":0.340}]}"#;
    let parsed = parse(below_limit, FoodKind::Foundation)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or("LOQ total sugars did not produce a profile")?;
    let reported = profile.carbohydrates.sugars.total.as_ref();
    assert_eq!(display(reported), Some("< 0.34 mg".to_owned()));
    assert_eq!(
        reported.map(Reported::qualifier),
        Some(crate::nutrition::Qualifier::LessThan)
    );
    Ok(())
}

#[test]
fn absent_or_only_unmodeled_nutrients_produce_no_profile() -> Result<(), Box<dyn std::error::Error>>
{
    for input in [
        r#"{}"#.to_owned(),
        food(&[
            nutrient(9000, "g", "1"),
            nutrient(9001, "g", "5"),
            r#"{"amount":4}"#.to_owned(),
        ]),
    ] {
        let parsed = parse(&input, FoodKind::Foundation)?;
        assert_eq!(parsed.nutrition, None);
    }
    Ok(())
}

#[test]
fn zero_with_positive_loq_becomes_less_than_threshold() -> Result<(), Box<dyn std::error::Error>> {
    let input = r#"{
        "foodNutrients":[
          {"nutrient":{"id":1003,"unitName":"G"},"amount":0,"loq":0.04},
          {"nutrient":{"id":1004,"unitName":"g"},"amount":0}
        ]
    }"#;
    let parsed = parse(input, FoodKind::Foundation)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or("zero nutrient profile was not parsed")?;
    assert_eq!(
        display(profile.protein.as_ref()),
        Some("< 0.04 g".to_owned())
    );
    assert_eq!(display(profile.fat.total.as_ref()), Some("0 g".to_owned()));
    Ok(())
}

#[test]
fn invalid_loq_is_reported_only_when_used() {
    let invalid_used = r#"{
        "foodNutrients":[
          {"nutrient":{"id":1003,"unitName":"g"},"amount":0,"loq":0.0000000000000000001}
        ]
    }"#;
    assert!(matches!(
        parse(invalid_used, FoodKind::Foundation),
        Err(NutritionError::Nutrient {
            id: 1003,
            source: MeasurementError::PrecisionLoss,
        })
    ));

    let ignored_nonzero_loq = r#"{
        "foodNutrients":[
          {"nutrient":{"id":1003,"unitName":"g"},"amount":1,"loq":0.0000000000000000001}
        ]
    }"#;
    assert!(parse(ignored_nonzero_loq, FoodKind::Foundation).is_ok());
}

#[test]
fn retains_multiple_measured_food_portions_as_full_gram_weights()
-> Result<(), Box<dyn std::error::Error>> {
    let input = r#"{
        "foodPortions":[
          {"amount":2.0,"measureUnit":{"name":"tablespoon"},"modifier":"heaped","gramWeight":33.9},
          {"amount":1,"measureUnit":{"name":"slice"},"portionDescription":"one slice","gramWeight":27},
          {"amount":1,"measureUnit":{"name":"cup"},"gramWeight":null}
        ]
    }"#;
    let parsed = parse(input, FoodKind::Survey)?;
    assert_eq!(parsed.serving_sizes.len(), 2);
    let first = parsed
        .serving_sizes
        .first()
        .ok_or("first measured serving size was not parsed")?;
    assert_eq!(first.name, "2 tablespoon heaped");
    assert_eq!(first.quantity.to_string(), "33.9 g");
    let second = parsed
        .serving_sizes
        .get(1)
        .ok_or("second measured serving size was not parsed")?;
    assert_eq!(second.name, "one slice");
    assert_eq!(second.quantity.to_string(), "27 g");
    Ok(())
}

#[test]
fn rejects_an_explicit_nonpositive_food_portion_weight() {
    let input = r#"{"foodPortions":[{"gramWeight":0,"measureUnit":{"name":"cup"}}]}"#;
    assert!(matches!(
        parse(input, FoodKind::Foundation),
        Err(NutritionError::Serving {
            source: MeasurementError::NonPositive,
        })
    ));
}

#[test]
fn branded_label_values_are_normalized_only_for_a_measured_serving()
-> Result<(), Box<dyn std::error::Error>> {
    let measured = r#"{
        "servingSize":28,"servingSizeUnit":"G","householdServingFullText":"1 bar",
        "labelNutrients":{
          "calories":{"value":126},"fat":{"value":7},"sodium":{"value":140}
        }
    }"#;
    let parsed = parse(measured, FoodKind::Branded)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or("measured label values were not normalized")?;
    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(
        display(profile.energy.as_ref()),
        Some("450 kcal".to_owned())
    );
    assert_eq!(display(profile.fat.total.as_ref()), Some("25 g".to_owned()));
    assert_eq!(display(profile.sodium.as_ref()), Some("500 mg".to_owned()));
    assert_eq!(profile.protein, None);
    let serving = parsed
        .serving_sizes
        .first()
        .ok_or("branded serving size was not parsed")?;
    assert_eq!(serving.name, "1 bar");

    let unmeasured = r#"{
        "servingSize":1,"servingSizeUnit":"cup",
        "labelNutrients":{"calories":{"value":120}}
    }"#;
    let parsed = parse(unmeasured, FoodKind::Branded)?;
    assert_eq!(parsed.nutrition, None);
    assert!(parsed.serving_sizes.is_empty());
    Ok(())
}

#[test]
fn standardized_branded_nutrients_use_explicit_measured_labels()
-> Result<(), Box<dyn std::error::Error>> {
    let input = r#"{
        "servingSize":100,"servingSizeUnit":"g",
        "foodNutrients":[
          {"nutrient":{"id":1004,"unitName":"g"},"amount":32.14},
          {"nutrient":{"id":1008,"unitName":"kcal"},"amount":500},
          {"nutrient":{"id":1003,"unitName":"g"},"amount":7}
        ],
        "labelNutrients":{"fat":{"value":9},"calories":{"value":140}}
    }"#;
    let parsed = parse(input, FoodKind::Branded)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or("measured label values were not normalized")?;
    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(display(profile.fat.total.as_ref()), Some("9 g".to_owned()));
    assert_eq!(
        display(profile.energy.as_ref()),
        Some("140 kcal".to_owned())
    );
    assert_eq!(profile.protein, None);
    Ok(())
}

#[test]
fn standardized_branded_nutrients_without_usable_labels_keep_the_basis_error() {
    let input = r#"{
        "servingSize":28,"servingSizeUnit":"g",
        "foodNutrients":[{"nutrient":{"id":1004,"unitName":"g"},"amount":32.14}]
    }"#;
    assert!(matches!(
        parse(input, FoodKind::Branded),
        Err(NutritionError::UnknownBrandedBasis)
    ));
}

#[test]
fn label_normalization_requires_exact_results() -> Result<(), Box<dyn std::error::Error>> {
    let nonrepresentable = r#"{
        "servingSize":28,"servingSizeUnit":"g",
        "labelNutrients":{"calories":{"value":120}}
    }"#;
    assert!(matches!(
        parse(nonrepresentable, FoodKind::Branded),
        Err(NutritionError::Nutrient {
            id: 1008,
            source: MeasurementError::PrecisionLoss,
        })
    ));

    let exact = r#"{
        "servingSize":28,"servingSizeUnit":"g",
        "labelNutrients":{"calories":{"value":126}}
    }"#;
    let parsed = parse(exact, FoodKind::Branded)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or("exact label value was not normalized")?;
    assert_eq!(
        display(profile.energy.as_ref()),
        Some("450 kcal".to_owned())
    );
    Ok(())
}

#[test]
fn label_volume_basis_is_not_treated_as_mass() -> Result<(), Box<dyn std::error::Error>> {
    let input = r#"{
        "servingSize":250,"servingSizeUnit":"ml",
        "foodNutrients":[{"nutrient":{"id":1004,"unitName":"g"},"amount":40}],
        "labelNutrients":{"calories":{"value":120},"fat":{"value":2}}
    }"#;
    let parsed = parse(input, FoodKind::Branded)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or("volume label values were not normalized")?;
    assert_eq!(profile.per, NutritionBasis::Per100Millilitres);
    assert_eq!(display(profile.energy.as_ref()), Some("48 kcal".to_owned()));
    assert_eq!(
        display(profile.fat.total.as_ref()),
        Some("0.8 g".to_owned())
    );
    let serving = parsed
        .serving_sizes
        .first()
        .ok_or("volume serving size was not parsed")?;
    assert_eq!(serving.quantity.to_string(), "250 ml");
    Ok(())
}

#[test]
fn invalid_standardized_branded_amounts_are_not_hidden_by_labels() {
    let input = r#"{
        "servingSize":100,"servingSizeUnit":"g",
        "foodNutrients":[{"nutrient":{"id":1004,"unitName":"g"},"amount":-1}],
        "labelNutrients":{"fat":{"value":9}}
    }"#;
    assert!(matches!(
        parse(input, FoodKind::Branded),
        Err(NutritionError::Nutrient {
            id: 1004,
            source: MeasurementError::InvalidDecimal,
        })
    ));
}

#[test]
fn invalid_supported_branded_serving_is_a_typed_error() {
    let input = r#"{"servingSize":0,"servingSizeUnit":"g"}"#;
    assert!(matches!(
        parse(input, FoodKind::Branded),
        Err(NutritionError::Serving {
            source: MeasurementError::NonPositive,
        })
    ));
}

fn food(nutrients: &[String]) -> String {
    format!(r#"{{"foodNutrients":[{}]}}"#, nutrients.join(","))
}

fn nutrient(id: NutrientId, unit: &str, amount: &str) -> String {
    format!(r#"{{"nutrient":{{"id":{id},"unitName":"{unit}"}},"amount":{amount}}}"#)
}

fn display<T: std::fmt::Display>(value: Option<&T>) -> Option<String> {
    value.map(ToString::to_string)
}
