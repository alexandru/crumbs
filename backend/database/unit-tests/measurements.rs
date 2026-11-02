use super::*;

#[test]
fn parses_decimal_and_scientific_notation_exactly() -> Result<(), Box<dyn std::error::Error>> {
    let decimal = parse_decimal("0.10000000000000001")?;
    let expected = Decimal::from_str_exact("0.10000000000000001")?;
    assert_eq!(decimal, expected);
    assert_eq!(parse_decimal("1.23e2")?, Decimal::from_str_exact("123")?);
    assert_eq!(
        parse_decimal("1.23e-2")?,
        Decimal::from_str_exact("0.0123")?
    );
    assert_eq!(parse_decimal("100e-2")?, Decimal::from_str_exact("1")?);
    assert_eq!(parse_decimal("0.0")?.to_string(), "0.0");
    assert_eq!(
        parse_decimal("1e-28")?,
        Decimal::from_str_exact("0.0000000000000000000000000001")?
    );
    Ok(())
}

#[test]
fn rejects_decimal_scale_range_and_precision_loss() {
    for input in ["", "1e", "1,2", " 1", "1 "] {
        assert!(matches!(
            parse_decimal(input),
            Err(MeasurementError::InvalidDecimalSyntax { .. })
        ));
    }
    assert!(matches!(
        parse_decimal("1e-29"),
        Err(MeasurementError::DecimalRange { .. })
    ));
    assert!(matches!(
        parse_decimal("1e29"),
        Err(MeasurementError::DecimalRange { .. })
    ));
    assert!(matches!(
        parse_decimal("1.234567890123456789012345678901"),
        Err(MeasurementError::DecimalRange { .. })
    ));
    assert!(matches!(
        parse_decimal("79228162514264337593543950336"),
        Err(MeasurementError::DecimalRange { .. })
    ));
    assert!(matches!(
        parse_decimal("1.00000000000000000000000000001"),
        Err(MeasurementError::DecimalRange { .. })
    ));
}

#[test]
fn enforces_nonnegative_quantities_and_positive_servings() -> Result<(), Box<dyn std::error::Error>>
{
    let zero = Grams::new(Decimal::ZERO)?;
    assert_eq!(zero.value(), Decimal::ZERO);
    assert!(matches!(
        Grams::new(parse_decimal("-0.1")?),
        Err(MeasurementError::NegativeQuantity)
    ));
    assert!(matches!(
        "-1 g".parse::<Mass>(),
        Err(MeasurementError::NegativeQuantity)
    ));
    assert!(matches!(
        "-1 kcal".parse::<Energy>(),
        Err(MeasurementError::NegativeQuantity)
    ));
    assert!(matches!(
        "0 g".parse::<ServingQuantity>(),
        Err(MeasurementError::NonPositiveServing)
    ));
    assert!(matches!(
        "-1 ml".parse::<ServingQuantity>(),
        Err(MeasurementError::NegativeQuantity)
    ));
    Ok(())
}

#[test]
fn preserves_signed_source_mass_and_normalizes_negative_zero()
-> Result<(), Box<dyn std::error::Error>> {
    let value = "-0.475 g".parse::<SignedMass>()?;
    assert!(value.is_negative());
    assert_eq!(
        value.magnitude(),
        Mass::Grams(Grams::new(parse_decimal("0.475")?)?)
    );
    assert_eq!(value.to_string(), "-0.475 g");
    let serialized = serde_json::to_string(&value)?;
    assert_eq!(serialized, "\"-0.475 g\"");
    assert_eq!(serde_json::from_str::<SignedMass>(&serialized)?, value);

    for input in ["-0.1 mg", "-0.25 µg"] {
        let parsed = input.parse::<SignedMass>()?;
        assert!(parsed.is_negative());
        assert_eq!(parsed.to_string(), input);
    }

    let reported = "< -0.1 g".parse::<Reported<SignedMass>>()?;
    assert_eq!(reported.qualifier, AmountQualifier::LessThan);
    assert_eq!(reported.to_string(), "< -0.1 g");

    let zero = "-0 g".parse::<SignedMass>()?;
    assert!(!zero.is_negative());
    assert_eq!(zero.to_string(), "0 g");
    Ok(())
}

#[test]
fn rejects_invalid_signed_mass_syntax_and_units() {
    assert!(matches!(
        "-1 kcal".parse::<SignedMass>(),
        Err(MeasurementError::IncompatibleUnit { .. })
    ));
    for input in ["-", "--1 g", "-+1 g"] {
        assert!(input.parse::<SignedMass>().is_err());
    }
}

#[test]
fn preserves_typed_mass_energy_and_serving_units() -> Result<(), Box<dyn std::error::Error>> {
    let quantities = ["1.25 g", "2 mg", "3 µg", "4 μg", "5 ug"];
    for quantity in quantities {
        let parsed = quantity.parse::<Mass>()?;
        let serialized = serde_json::to_string(&parsed)?;
        let restored = serde_json::from_str::<Mass>(&serialized)?;
        assert_eq!(restored, parsed);
    }

    let canonical = "5 ug".parse::<Mass>()?.to_string();
    assert_eq!(canonical, "5 µg");
    for quantity in ["6 kcal", "7 kJ"] {
        let parsed = quantity.parse::<Energy>()?;
        let serialized = serde_json::to_string(&parsed)?;
        let restored = serde_json::from_str::<Energy>(&serialized)?;
        assert_eq!(restored, parsed);
    }

    for quantity in ["8 g", "9 mg", "10 µg", "11 ml", "12 l"] {
        let parsed = quantity.parse::<ServingQuantity>()?;
        let serialized = serde_json::to_string(&parsed)?;
        let restored = serde_json::from_str::<ServingQuantity>(&serialized)?;
        assert_eq!(restored, parsed);
    }

    let expected = Decimal::from_str_exact("250")?;
    let volume = "250 ml".parse::<ServingQuantity>()?;
    assert_eq!(
        volume.measure(),
        ServingMeasure::Millilitres(Millilitres::new(expected)?)
    );
    Ok(())
}

#[test]
fn preserves_all_reported_qualifiers_in_string_serde() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        ("1 g", AmountQualifier::Exact),
        ("< 1 g", AmountQualifier::LessThan),
        ("<= 1 g", AmountQualifier::LessThanOrEqual),
        ("> 1 g", AmountQualifier::GreaterThan),
        (">= 1 g", AmountQualifier::GreaterThanOrEqual),
        ("~ 1 g", AmountQualifier::Approximate),
    ];

    for (text, qualifier) in cases {
        let parsed = text.parse::<Reported<Mass>>()?;
        assert_eq!(parsed.qualifier, qualifier);
        assert_eq!(parsed.to_string(), text);
        let serialized = serde_json::to_string(&parsed)?;
        let restored = serde_json::from_str::<Reported<Mass>>(&serialized)?;
        assert_eq!(restored, parsed);
    }
    let inclusive_alias = "≤ 1 g".parse::<Reported<Mass>>()?;
    assert_eq!(inclusive_alias.qualifier, AmountQualifier::LessThanOrEqual);
    assert_eq!(inclusive_alias.to_string(), "<= 1 g");
    let greater_alias = "≥ 1 g".parse::<Reported<Mass>>()?;
    assert_eq!(greater_alias.qualifier, AmountQualifier::GreaterThanOrEqual);
    assert_eq!(greater_alias.to_string(), ">= 1 g");
    Ok(())
}

#[test]
fn rejects_wrong_units_and_numeric_json_values() {
    assert!(matches!(
        "1 mg".parse::<Grams>(),
        Err(MeasurementError::IncompatibleUnit { .. })
    ));
    assert!(matches!(
        "1 oz".parse::<Mass>(),
        Err(MeasurementError::UnsupportedUnit { .. })
    ));
    assert!(matches!(
        "1 g".parse::<Energy>(),
        Err(MeasurementError::IncompatibleUnit { .. })
    ));
    assert!(serde_json::from_str::<Grams>("1.25").is_err());
    assert!(serde_json::from_str::<Mass>("1").is_err());
    assert!(serde_json::from_str::<ServingQuantity>("250").is_err());
}
