use super::*;

#[test]
fn parsed_food_data_and_profile_start_empty() {
    let parsed = ParsedFoodData::default();
    assert_eq!(parsed.nutrition, None);
    assert!(parsed.serving_sizes.is_empty());

    let profile = NutritionProfile::new(NutritionBasis::Per100Grams);
    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(profile.fat, Fat::default());
    assert_eq!(profile.carbohydrates, Carbohydrates::default());
    assert_eq!(profile.energy, None);
    assert_eq!(profile.sodium, None);
}

#[test]
fn measurements_preserve_units_and_reject_dimension_mismatches() -> Result<(), MeasurementError> {
    assert_eq!("0 g".parse::<Mass>()?.to_string(), "0 g");
    assert_eq!("12.5 mg".parse::<Mass>()?.to_string(), "12.5 mg");
    assert_eq!("4 ug".parse::<Mass>()?.to_string(), "4 µg");
    assert_eq!("4 µg".parse::<Mass>()?.to_string(), "4 µg");
    assert_eq!("15 kcal".parse::<Energy>()?.to_string(), "15 kcal");
    assert_eq!("63 kJ".parse::<Energy>()?.to_string(), "63 kJ");
    assert_eq!("250 ml".parse::<Millilitres>()?.to_string(), "250 ml");

    assert!(matches!(
        "1 mg".parse::<Grams>(),
        Err(MeasurementError::InvalidUnit)
    ));
    assert!(matches!(
        "1 kcal".parse::<Mass>(),
        Err(MeasurementError::IncompatibleDimension)
    ));
    assert!(matches!(
        "1 ml".parse::<Energy>(),
        Err(MeasurementError::IncompatibleDimension)
    ));
    assert!(matches!(
        "1 g".parse::<Kilocalories>(),
        Err(MeasurementError::IncompatibleDimension)
    ));
    Ok(())
}

#[test]
fn reported_qualifiers_roundtrip_in_text_and_json() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        ("2 g", Qualifier::Exact),
        ("< 2 g", Qualifier::LessThan),
        ("<= 2 g", Qualifier::LessThanOrEqual),
        ("> 2 g", Qualifier::GreaterThan),
        (">= 2 g", Qualifier::GreaterThanOrEqual),
        ("~ 2 g", Qualifier::Approximate),
    ];

    for (text, qualifier) in cases {
        let reported = text.parse::<Reported<Mass>>()?;
        assert_eq!(reported.qualifier(), qualifier);
        assert_eq!(reported.to_string(), text);
        assert_eq!(
            serde_json::to_string(&reported)?,
            serde_json::to_string(text)?
        );
        assert_eq!(
            serde_json::from_str::<Reported<Mass>>(&serde_json::to_string(text)?)?,
            reported
        );
    }

    assert_eq!(
        "<= 42 kJ".parse::<Reported<Energy>>()?.to_string(),
        "<= 42 kJ"
    );
    assert_eq!(
        "≤ 42 kJ".parse::<Reported<Energy>>()?.to_string(),
        "<= 42 kJ"
    );
    assert_eq!(
        "≥ 42 kJ".parse::<Reported<Energy>>()?.to_string(),
        ">= 42 kJ"
    );
    Ok(())
}

#[test]
fn decimal_parsing_is_exact_and_supports_exponents() -> Result<(), MeasurementError> {
    let exact = "0.100000000000000001".parse::<Decimal>()?;
    assert_eq!(exact.to_string(), "0.100000000000000001");
    assert_eq!("1e3".parse::<Decimal>()?.to_string(), "1000");
    assert_eq!("1.25e-2".parse::<Decimal>()?.to_string(), "0.0125");
    assert_eq!("1.0000000000000000000".parse::<Decimal>()?.to_string(), "1");
    assert!("0".parse::<Decimal>()?.is_zero());
    assert!("0 g".parse::<Grams>()?.0.is_zero());
    Ok(())
}

#[test]
fn decimal_precision_range_and_invalid_values_are_typed() -> Result<(), MeasurementError> {
    assert!(matches!(
        "0.0000000000000000001".parse::<Decimal>(),
        Err(MeasurementError::PrecisionLoss)
    ));
    assert!(matches!(
        "-0".parse::<Decimal>(),
        Err(MeasurementError::InvalidDecimal)
    ));
    assert!(matches!(
        "NaN".parse::<Decimal>(),
        Err(MeasurementError::InvalidDecimal)
    ));
    assert!(matches!(
        "inf".parse::<Decimal>(),
        Err(MeasurementError::InvalidDecimal)
    ));
    assert!(matches!(
        "1e100".parse::<Decimal>(),
        Err(MeasurementError::OutOfRange)
    ));
    let maximum = "340282366920938463463.374607431768211455".parse::<Decimal>()?;
    assert_eq!(
        maximum.to_string(),
        "340282366920938463463.374607431768211455"
    );
    assert!(matches!(
        "340282366920938463463.374607431768211456".parse::<Decimal>(),
        Err(MeasurementError::OutOfRange)
    ));
    Ok(())
}

#[test]
fn serving_quantities_are_positive_and_keep_mass_and_volume_distinct()
-> Result<(), MeasurementError> {
    let grams = "28 g".parse::<ServingQuantity>()?;
    assert_eq!(grams.to_string(), "28 g");
    assert_eq!(grams.basis(), NutritionBasis::Per100Grams);
    let milligrams = "500 mg".parse::<ServingQuantity>()?;
    let micrograms = "500000 µg".parse::<ServingQuantity>()?;
    let volume = "250 ml".parse::<ServingQuantity>()?;
    assert_eq!(milligrams.basis(), NutritionBasis::Per100Grams);
    assert_eq!(micrograms.basis(), NutritionBasis::Per100Grams);
    assert_eq!(volume.basis(), NutritionBasis::Per100Millilitres);
    assert_eq!(milligrams.hundred_ratio()?.1.to_string(), "0.5");
    assert_eq!(micrograms.hundred_ratio()?.1.to_string(), "0.5");
    assert_eq!(volume.hundred_ratio()?.1.to_string(), "250");

    assert!(matches!(
        "0 g".parse::<ServingQuantity>(),
        Err(MeasurementError::NonPositive)
    ));
    assert!(matches!(
        "0 ml".parse::<ServingQuantity>(),
        Err(MeasurementError::NonPositive)
    ));
    assert!(matches!(
        "< 28 g".parse::<ServingQuantity>(),
        Err(MeasurementError::QualifierNotAllowed)
    ));
    assert!(matches!(
        "5 kcal".parse::<ServingQuantity>(),
        Err(MeasurementError::IncompatibleDimension)
    ));
    Ok(())
}

#[test]
fn carbohydrates_keep_including_and_excluding_fibre_distinct()
-> Result<(), Box<dyn std::error::Error>> {
    let carbohydrates: Carbohydrates = serde_json::from_str(
        r#"{"including_fibre":null,"excluding_fibre":"31 g","sugars":{"total":"< 8 g"}}"#,
    )?;
    assert_eq!(carbohydrates.including_fibre, None);
    assert_eq!(
        carbohydrates
            .excluding_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("31 g")
    );
    assert_eq!(
        carbohydrates
            .sugars
            .total
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("< 8 g")
    );
    assert_eq!(carbohydrates.sugars.added, None);
    Ok(())
}

#[test]
fn off_like_profile_json_roundtrips_volume_units_and_optional_data()
-> Result<(), Box<dyn std::error::Error>> {
    let json = r#"
        {
          "nutrition": {
            "per": "100 ml",
            "energy": "~ 120 kcal",
            "fat": {"total": "1.2 g", "saturated": "< 0.4 g"},
            "carbohydrates": {
              "excluding_fibre": "4.8 g",
              "sugars": {"total": "3.1 g", "added": null}
            },
            "protein": "0.7 g",
            "sodium": "18 mg",
            "cholesterol": "25 µg"
          },
          "serving_sizes": [{"name": "glass", "quantity": "250 ml"}]
        }
    "#;

    let parsed: ParsedFoodData = serde_json::from_str(json)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing nutrition"))?;
    assert_eq!(profile.per, NutritionBasis::Per100Millilitres);
    assert_eq!(
        profile.energy.as_ref().map(ToString::to_string).as_deref(),
        Some("~ 120 kcal")
    );
    assert_eq!(
        profile
            .fat
            .total
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("1.2 g")
    );
    assert_eq!(
        profile
            .fat
            .saturated
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("< 0.4 g")
    );
    assert_eq!(profile.carbohydrates.including_fibre, None);
    assert_eq!(
        profile
            .carbohydrates
            .excluding_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("4.8 g")
    );
    assert_eq!(
        profile.sodium.as_ref().map(ToString::to_string).as_deref(),
        Some("18 mg")
    );
    assert_eq!(
        profile
            .cholesterol
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("25 µg")
    );
    assert_eq!(
        parsed
            .serving_sizes
            .first()
            .map(|serving| serving.quantity.to_string())
            .as_deref(),
        Some("250 ml")
    );

    let encoded = serde_json::to_string(&parsed)?;
    let decoded: ParsedFoodData = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, parsed);
    Ok(())
}

#[test]
fn open_food_facts_milk_roundtrips_explicit_volume_nutrition()
-> Result<(), Box<dyn std::error::Error>> {
    // Primary source: https://world.openfoodfacts.org/api/v2/product/8410297110986.json
    let json = r#"{"nutrition":{"per":"100 ml","energy":"45 kcal","carbohydrates":{"excluding_fibre":"4.65 g","sugars":{"total":"4.65 g","lactose":null}},"fibre":null,"salt":"1 g","sodium":"0.4 g"},"serving_sizes":[{"name":"1 serving (100 ml)","quantity":"100 ml"}]}"#;
    let parsed: ParsedFoodData = serde_json::from_str(json)?;
    let profile = parsed
        .nutrition
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing nutrition"))?;

    assert_eq!(profile.per, NutritionBasis::Per100Millilitres);
    assert_eq!(profile.carbohydrates.including_fibre, None);
    assert_eq!(
        profile
            .carbohydrates
            .excluding_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("4.65 g")
    );
    assert_eq!(
        profile
            .carbohydrates
            .sugars
            .total
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("4.65 g")
    );
    assert_eq!(profile.fibre, None);
    assert_eq!(profile.carbohydrates.sugars.lactose, None);
    assert_eq!(
        profile.energy.as_ref().map(ToString::to_string).as_deref(),
        Some("45 kcal")
    );
    assert_eq!(
        profile.salt.as_ref().map(ToString::to_string).as_deref(),
        Some("1 g")
    );
    assert_eq!(
        profile.sodium.as_ref().map(ToString::to_string).as_deref(),
        Some("0.4 g")
    );
    assert_eq!(
        parsed
            .serving_sizes
            .first()
            .map(|serving| serving.quantity.to_string())
            .as_deref(),
        Some("100 ml")
    );

    let encoded = serde_json::to_string(&parsed)?;
    let decoded: ParsedFoodData = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, parsed);
    Ok(())
}

#[test]
fn open_food_facts_nutella_roundtrips_approximate_sodium_and_gram_units()
-> Result<(), Box<dyn std::error::Error>> {
    // Primary source: https://world.openfoodfacts.org/api/v2/product/3017620422003.json
    let json = r#"{"nutrition":{"per":"100 g","sodium":"~ 0.0428 g","salt":"0.107 g"}}"#;
    let parsed: ParsedFoodData = serde_json::from_str(json)?;
    let encoded = serde_json::to_string(&parsed)?;
    let decoded: ParsedFoodData = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, parsed);

    let profile = decoded
        .nutrition
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing nutrition"))?;
    let sodium = profile
        .sodium
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing sodium"))?;
    assert_eq!(sodium.qualifier(), Qualifier::Approximate);
    assert!(matches!(sodium.amount(), Mass::Grams(_)));
    assert_eq!(sodium.to_string(), "~ 0.0428 g");
    let salt = profile
        .salt
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing salt"))?;
    assert!(matches!(salt.amount(), Mass::Grams(_)));
    assert_eq!(salt.to_string(), "0.107 g");
    Ok(())
}

#[test]
fn exact_ratio_reduces_factors_and_rejects_nonrepresentable_results() -> Result<(), MeasurementError>
{
    let value = "6".parse::<Decimal>()?;
    let numerator = "0.5".parse::<Decimal>()?;
    let denominator = "1.5".parse::<Decimal>()?;
    assert_eq!(
        value.checked_ratio(numerator, denominator)?.to_string(),
        "2"
    );
    assert!(matches!(
        "1".parse::<Decimal>()?
            .checked_ratio("1".parse()?, "3".parse()?),
        Err(MeasurementError::PrecisionLoss)
    ));
    let one = "1".parse::<Decimal>()?;
    let three = "3".parse::<Decimal>()?;
    let zero = "0".parse::<Decimal>()?;
    assert_eq!(zero.checked_ratio(one, three)?.to_string(), "0");
    assert_eq!(one.checked_ratio(zero, three)?.to_string(), "0");
    assert!(matches!(
        one.checked_ratio(one, zero),
        Err(MeasurementError::ConversionFailed)
    ));

    let maximum = "340282366920938463463.374607431768211455".parse::<Decimal>()?;
    assert_eq!(maximum.checked_ratio(one, one)?, maximum);
    assert!(matches!(
        maximum.checked_ratio("2".parse()?, one),
        Err(MeasurementError::OutOfRange)
    ));

    let reported = "< 120 kcal".parse::<Reported<Energy>>()?;
    let normalized = reported.checked_ratio("1".parse()?, "2".parse()?)?;
    assert_eq!(normalized.to_string(), "< 60 kcal");
    let threshold = "< 0.000000000000000001 g".parse::<Reported<Mass>>()?;
    assert!(matches!(
        threshold.checked_ratio(one, "2".parse()?),
        Err(MeasurementError::PrecisionLoss)
    ));
    let approximate = "~ 0.000000000000000001 kcal".parse::<Reported<Energy>>()?;
    assert!(matches!(
        approximate.checked_ratio("1".parse()?, "2".parse()?),
        Err(MeasurementError::PrecisionLoss)
    ));
    Ok(())
}

#[test]
fn serde_rejects_invalid_measurement_boundaries() {
    let cases = [
        (
            "negative serving",
            r#"{"serving_sizes":[{"name":"serving","quantity":"-1 g"}]}"#,
        ),
        (
            "zero serving",
            r#"{"serving_sizes":[{"name":"serving","quantity":"0 g"}]}"#,
        ),
        (
            "incompatible nutrient unit",
            r#"{"nutrition":{"per":"100 g","protein":"1 kcal"}}"#,
        ),
        (
            "excess decimal precision",
            r#"{"nutrition":{"per":"100 g","protein":"0.0000000000000000001 g"}}"#,
        ),
        (
            "decimal range",
            r#"{"nutrition":{"per":"100 g","protein":"340282366920938463463.374607431768211456 g"}}"#,
        ),
        ("unsupported basis", r#"{"nutrition":{"per":"100kg"}}"#),
    ];
    for (case, json) in cases {
        assert!(
            serde_json::from_str::<ParsedFoodData>(json).is_err(),
            "{case}"
        );
    }
}

#[test]
fn unit_bearing_decimal_roundtrips_exactly_through_json() -> Result<(), Box<dyn std::error::Error>>
{
    let json = r#"{"nutrition":{"per":"100 g","protein":"0.100000000000000001 g"}}"#;
    let parsed: ParsedFoodData = serde_json::from_str(json)?;
    let encoded = serde_json::to_string(&parsed)?;
    let decoded: ParsedFoodData = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, parsed);
    assert_eq!(
        decoded
            .nutrition
            .as_ref()
            .and_then(|profile| profile.protein.as_ref())
            .map(ToString::to_string)
            .as_deref(),
        Some("0.100000000000000001 g")
    );
    Ok(())
}
