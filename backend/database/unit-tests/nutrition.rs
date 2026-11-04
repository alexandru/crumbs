use std::error::Error;
use std::str::FromStr;

use serde_json::json;

use super::*;

#[test]
fn food_data_round_trips_mass_and_volume_profiles_without_defaulting_missing_amounts()
-> Result<(), Box<dyn Error>> {
    let mass_data: FoodData = serde_json::from_value(json!({
        "nutrition": {
            "per": "100 g",
            "energy": "210 kcal",
            "fat": { "total": "8.4 g", "saturated": "1.2 g", "trans": "< 0.1 g" },
            "carbohydrates": {
                "including_fibre": "28.5 g",
                "excluding_fibre": null,
                "sugars": { "total": "4.2 g", "added": "1.0 g", "lactose": "0.55 g" },
                "starch": null
            },
            "fibre": "4.0 g",
            "protein": "7.0 g",
            "sodium": "150 mg",
            "salt": null,
            "cholesterol": null
        },
        "serving_sizes": []
    }))?;
    let encoded = serde_json::to_value(&mass_data)?;
    let decoded: FoodData = serde_json::from_value(encoded)?;
    assert_eq!(decoded, mass_data);
    let profile = mass_data.nutrition.as_ref().ok_or("profile is missing")?;
    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(
        profile
            .carbohydrates
            .including_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("28.5 g")
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
        Some("0.55 g")
    );
    assert_eq!(
        profile.sodium.as_ref().map(ToString::to_string).as_deref(),
        Some("150 mg")
    );
    assert_eq!(profile.salt, None);

    let volume_data: FoodData = serde_json::from_value(json!({
        "nutrition": { "per": "100 ml" },
        "serving_sizes": []
    }))?;
    let volume_profile = volume_data
        .nutrition
        .as_ref()
        .ok_or("volume profile is missing")?;
    assert_eq!(volume_profile.per, NutritionBasis::Per100Millilitres);
    assert_eq!(volume_profile.energy, None);
    assert_eq!(volume_profile.fat.total, None);
    assert_eq!(volume_profile.carbohydrates.sugars.lactose, None);
    assert_eq!(volume_profile.sodium, None);
    assert_eq!(volume_profile.salt, None);
    Ok(())
}

#[test]
fn represents_absent_profile_partial_profile_and_reported_zero_separately()
-> Result<(), Box<dyn Error>> {
    let absent: FoodData =
        serde_json::from_value(json!({ "nutrition": null, "serving_sizes": [] }))?;
    assert_eq!(absent.nutrition, None);

    let partial: FoodData = serde_json::from_value(json!({
        "nutrition": { "per": "100 g", "protein": "0 g", "salt": "0.2 g" },
        "serving_sizes": []
    }))?;
    let profile = partial
        .nutrition
        .as_ref()
        .ok_or("partial profile is missing")?;
    assert_eq!(
        profile.protein.as_ref().map(ToString::to_string).as_deref(),
        Some("0 g")
    );
    assert_eq!(profile.sodium, None);
    assert_eq!(
        profile.salt.as_ref().map(ToString::to_string).as_deref(),
        Some("0.2 g")
    );
    assert_eq!(profile.carbohydrates.including_fibre, None);
    assert_eq!(profile.carbohydrates.excluding_fibre, None);
    Ok(())
}

#[test]
fn reported_values_round_trip_exact_decimal_values_units_and_qualifiers()
-> Result<(), Box<dyn Error>> {
    let values = [
        (
            "0.1000000000000000000000000001 g",
            "0.1000000000000000000000000001 g",
        ),
        ("< 0.5 g", "< 0.5 g"),
        ("<= 15 mg", "<= 15 mg"),
        ("> 0 µg", "> 0 µg"),
        (">= 0.55 µg", ">= 0.55 µg"),
        ("~ 15.00 mg", "~ 15.00 mg"),
    ];
    for (input, expected) in values {
        let parsed = Reported::<Mass>::from_str(input)?;
        let serialized = serde_json::to_string(&parsed)?;
        assert_eq!(serialized, format!("\"{expected}\""));
        let decoded: Reported<Mass> = serde_json::from_str(&serialized)?;
        assert_eq!(decoded, parsed);
    }

    for (input, expected) in [("15 kcal", "15 kcal"), ("~ 0.55 kJ", "~ 0.55 kJ")] {
        let parsed = Reported::<Energy>::from_str(input)?;
        let serialized = serde_json::to_string(&parsed)?;
        assert_eq!(serialized, format!("\"{expected}\""));
        let decoded: Reported<Energy> = serde_json::from_str(&serialized)?;
        assert_eq!(decoded, parsed);
    }
    Ok(())
}

#[test]
fn rejects_negative_incompatible_unknown_and_unrepresentable_measurements()
-> Result<(), Box<dyn Error>> {
    for input in [
        "-1 g",
        "15 ml",
        "1 lb",
        "0.12345678901234567890123456789 g",
        "79228162514264337593543950336 g",
    ] {
        assert!(
            Reported::<Mass>::from_str(input).is_err(),
            "accepted invalid value {input}"
        );
    }
    assert!(Reported::<Energy>::from_str("15 g").is_err());
    assert!(Reported::<Energy>::from_str("1 kcal extra").is_err());
    assert!(serde_json::from_str::<Grams>("\"1 mg\"").is_err());
    assert!(serde_json::from_str::<Grams>("\"-1 g\"").is_err());
    assert!(serde_json::from_str::<Kilojoules>("\"1 kcal\"").is_err());
    Ok(())
}

#[test]
fn serving_sizes_are_positive_measured_quantities_and_keep_servings_separate_from_nutrition()
-> Result<(), Box<dyn Error>> {
    let data: FoodData = serde_json::from_value(json!({
        "nutrition": { "per": "100 g", "protein": "7 g" },
        "serving_sizes": [
            { "name": "one slice", "quantity": "30 g" },
            { "name": "two slices", "quantity": "60 g" },
            { "name": "one glass", "quantity": "240 ml" }
        ]
    }))?;
    assert_eq!(data.serving_sizes.len(), 3);
    assert!(
        data.serving_sizes
            .first()
            .and_then(|serving| serving.quantity.volume())
            .is_none()
    );
    assert!(
        data.serving_sizes
            .get(2)
            .and_then(|serving| serving.quantity.mass())
            .is_none()
    );
    assert_eq!(
        data.serving_sizes
            .first()
            .map(|serving| serving.quantity.to_string())
            .as_deref(),
        Some("30 g")
    );
    assert_eq!(
        data.serving_sizes
            .get(2)
            .map(|serving| serving.quantity.to_string())
            .as_deref(),
        Some("240 ml")
    );
    assert_eq!(
        data.nutrition
            .as_ref()
            .ok_or("profile is missing")?
            .protein
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("7 g")
    );
    for input in ["0 g", "0 ml", "-1 g", "1 kcal", "~ 1 g", "1 lb"] {
        assert!(
            MeasuredQuantity::from_str(input).is_err(),
            "accepted invalid serving quantity {input}"
        );
    }
    assert!(MeasuredQuantity::from_str("1 g").is_ok());
    assert!(MeasuredQuantity::from_str("1 ml").is_ok());
    Ok(())
}

#[test]
fn reported_values_cannot_encode_a_strict_zero_upper_bound() -> Result<(), Box<dyn Error>> {
    let zero = Mass::from_str("0 g")?;
    assert!(matches!(
        Reported::new(Qualifier::LessThan, zero),
        Err(QuantityError::ZeroStrictUpperBound)
    ));
    assert!(Reported::<Mass>::from_str("< 0 g").is_err());
    assert!(serde_json::from_str::<Reported<Mass>>("\"< 0 g\"").is_err());

    assert!(Reported::new(Qualifier::AtMost, zero).is_ok());
    assert!(Reported::new(Qualifier::GreaterThan, zero).is_ok());
    assert!(Reported::<Mass>::from_str("<= 0 g").is_ok());
    assert!(Reported::<Mass>::from_str("> 0 g").is_ok());
    Ok(())
}

#[test]
fn less_than_uses_a_positive_limit_proof() -> Result<(), Box<dyn Error>> {
    let limit = Positive::try_new(Mass::from_str("0.55 g")?)?;
    let reported = Reported::less_than(limit);

    assert_eq!(reported.qualifier(), Qualifier::LessThan);
    assert_eq!(reported.amount().to_string(), "0.55 g");
    Ok(())
}

#[test]
fn quantity_builder_parses_values_and_units_without_combining_strings() -> Result<(), Box<dyn Error>>
{
    let reported = Reported::<Mass>::parse_with_unit("< 0.55", "µg")?;
    assert_eq!(reported.qualifier(), Qualifier::LessThan);
    assert_eq!(reported.amount().to_string(), "0.55 µg");
    assert_eq!(reported.into_amount().to_string(), "0.55 µg");

    assert!(Reported::<Mass>::parse_with_unit("0", "g").is_ok());
    assert!(Reported::<Mass>::parse_with_unit("-0.1", "g").is_err());
    assert!(Reported::<Mass>::parse_with_unit("1", "kcal").is_err());
    Ok(())
}

#[test]
fn positive_measured_quantities_cannot_encode_zero() -> Result<(), Box<dyn Error>> {
    let zero_mass = Mass::from_str("0 g")?;
    let zero_volume = Millilitres::from_str("0 ml")?;
    assert!(matches!(
        Positive::try_new(zero_mass),
        Err(QuantityError::NonPositive)
    ));
    assert!(matches!(
        Positive::try_new(zero_volume),
        Err(QuantityError::NonPositive)
    ));

    for quantity in [
        MeasuredQuantity::from_str("0 g"),
        MeasuredQuantity::from_str("0 ml"),
    ] {
        assert!(quantity.is_err());
    }
    for quantity in ["1 g", "1 ml"] {
        let parsed = MeasuredQuantity::from_str(quantity)?;
        let encoded = serde_json::to_string(&parsed)?;
        let decoded: MeasuredQuantity = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, parsed);
    }
    assert!(serde_json::from_str::<MeasuredQuantity>("\"0 g\"").is_err());
    Ok(())
}

#[test]
fn representative_open_food_facts_values_fit_the_shared_model() -> Result<(), Box<dyn Error>> {
    let european: FoodData = serde_json::from_value(json!({
        "nutrition": {
            "per": "100 g",
            "energy": null,
            "carbohydrates": {
                "including_fibre": null,
                "excluding_fibre": "28.5 g",
                "sugars": { "total": "4.2 g", "added": null, "lactose": null },
                "starch": null
            },
            "fibre": "4.0 g"
        },
        "serving_sizes": []
    }))?;
    let us: FoodData = serde_json::from_value(json!({
        "nutrition": {
            "per": "100 g",
            "carbohydrates": { "including_fibre": "28.5 g" },
            "fat": { "total": "8.4 g" }
        },
        "serving_sizes": []
    }))?;
    let beverage: FoodData = serde_json::from_value(json!({
        "nutrition": { "per": "100 ml", "energy": "210 kJ", "sodium": "~ 150 mg", "fibre": "0.55 µg" },
        "serving_sizes": [{ "name": "one cup", "quantity": "250 ml" }]
    }))?;

    let european_profile = european
        .nutrition
        .as_ref()
        .ok_or("European profile is missing")?;
    assert_eq!(
        european_profile
            .carbohydrates
            .excluding_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("28.5 g")
    );
    assert_eq!(european_profile.carbohydrates.including_fibre, None);
    let us_profile = us.nutrition.as_ref().ok_or("US profile is missing")?;
    assert_eq!(
        us_profile
            .carbohydrates
            .including_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("28.5 g")
    );
    assert_eq!(us_profile.carbohydrates.excluding_fibre, None);
    let beverage_profile = beverage
        .nutrition
        .as_ref()
        .ok_or("beverage profile is missing")?;
    assert_eq!(beverage_profile.per, NutritionBasis::Per100Millilitres);
    assert_eq!(
        beverage_profile
            .energy
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("210 kJ")
    );
    assert_eq!(
        beverage_profile
            .sodium
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("~ 150 mg")
    );
    assert_eq!(
        beverage_profile
            .fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("0.55 µg")
    );
    assert_eq!(beverage.serving_sizes.len(), 1);
    Ok(())
}

#[test]
fn parses_scientific_decimal_notation_exactly_without_rounding() -> Result<(), Box<dyn Error>> {
    for (input, expected) in [("1e-3 g", "0.001 g"), ("1e2 g", "100 g")] {
        let parsed = Reported::<Mass>::from_str(input)?;
        assert_eq!(parsed.to_string(), expected);
    }
    for input in ["1e-29 g", "123456789012345678901234567890e-29 g"] {
        assert!(
            Reported::<Mass>::from_str(input).is_err(),
            "accepted unsupported precision {input}"
        );
    }
    Ok(())
}

#[test]
fn concrete_measurement_types_serialize_and_deserialize_with_their_own_units()
-> Result<(), Box<dyn Error>> {
    let cases = [
        ("\"1 g\"", serde_json::to_string(&"1 g")?),
        ("\"2 mg\"", serde_json::to_string(&"2 mg")?),
        ("\"3 µg\"", serde_json::to_string(&"3 µg")?),
        ("\"4 ml\"", serde_json::to_string(&"4 ml")?),
        ("\"5 kcal\"", serde_json::to_string(&"5 kcal")?),
        ("\"6 kJ\"", serde_json::to_string(&"6 kJ")?),
    ];
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<Grams>(cases[0].0)?)?,
        cases[0].1
    );
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<Milligrams>(cases[1].0)?)?,
        cases[1].1
    );
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<Micrograms>(cases[2].0)?)?,
        cases[2].1
    );
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<Millilitres>(cases[3].0)?)?,
        cases[3].1
    );
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<Kilocalories>(cases[4].0)?)?,
        cases[4].1
    );
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<Kilojoules>(cases[5].0)?)?,
        cases[5].1
    );
    Ok(())
}

#[test]
fn canonicalizes_inclusive_unicode_qualifiers() -> Result<(), Box<dyn Error>> {
    for (input, expected) in [("≤ 0.5 g", "<= 0.5 g"), ("≥ 15 mg", ">= 15 mg")] {
        let parsed = Reported::<Mass>::from_str(input)?;
        assert_eq!(parsed.to_string(), expected);
        let encoded = serde_json::to_string(&parsed)?;
        assert_eq!(encoded, format!("\"{expected}\""));
        let decoded: Reported<Mass> = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, parsed);
    }
    Ok(())
}
