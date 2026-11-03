use std::error::Error as StdError;

use serde_json::Number as JsonNumber;

use super::{
    Decimal, Energy, Grams, Kilocalories, Kilojoules, Mass, MassUnit, MeasureError, MeasuredMass,
    MeasuredQuantity, MeasuredVolume, Micrograms, Milligrams, Qualifier, decimal_from_json_number,
    measured_decimal_from_json_number,
};

type TestResult = Result<(), Box<dyn StdError>>;
type ParseResult<T> = Result<T, MeasureError>;

#[test]
fn parses_plain_unit_bearing_amounts() -> TestResult {
    let grams = Grams::parse("8.4 g")?;
    assert_eq!(grams.value(), Decimal::from_str_exact("8.4")?);
    assert_eq!(grams.qualifier(), None);
    assert_eq!(grams.to_string(), "8.4 g");

    let milligrams = Milligrams::parse("15 mg")?;
    assert_eq!(milligrams.to_string(), "15 mg");

    let micrograms = Micrograms::parse("0.55 µg")?;
    assert_eq!(micrograms.to_string(), "0.55 µg");

    let kilocalories = Kilocalories::parse("210 kcal")?;
    assert_eq!(kilocalories.to_string(), "210 kcal");

    let kilojoules = Kilojoules::parse("960 kJ")?;
    assert_eq!(kilojoules.to_string(), "960 kJ");
    Ok(())
}

#[test]
fn parses_qualified_amounts() -> TestResult {
    let less_than = Grams::parse("< 0.5 g")?;
    assert_eq!(less_than.qualifier(), Some(Qualifier::LessThan));
    assert_eq!(less_than.to_string(), "< 0.5 g");

    let approximate = Milligrams::parse("~ 15 mg")?;
    assert_eq!(approximate.qualifier(), Some(Qualifier::Approximately));
    assert_eq!(approximate.to_string(), "~ 15 mg");

    let at_most = Grams::parse("<= 3 g")?;
    assert_eq!(at_most.qualifier(), Some(Qualifier::AtMost));
    assert_eq!(at_most.to_string(), "<= 3 g");

    let at_least = Grams::parse("≥ 3 g")?;
    assert_eq!(at_least.qualifier(), Some(Qualifier::AtLeast));
    assert_eq!(at_least.to_string(), ">= 3 g");

    let greater_than = Grams::parse("> 0.1 g")?;
    assert_eq!(greater_than.qualifier(), Some(Qualifier::GreaterThan));
    assert_eq!(greater_than.to_string(), "> 0.1 g");
    Ok(())
}

#[test]
fn parses_mass_and_energy_enums_in_source_units() -> TestResult {
    assert_eq!(
        Mass::parse("150 mg")?,
        Mass::Milligrams(Milligrams::parse("150 mg")?)
    );
    assert_eq!(Mass::parse("8.4 g")?, Mass::Grams(Grams::parse("8.4 g")?));
    assert_eq!(
        Mass::parse("0.55 µg")?,
        Mass::Micrograms(Micrograms::parse("0.55 µg")?)
    );
    assert_eq!(
        Energy::parse("210 kcal")?,
        Energy::Kilocalories(Kilocalories::parse("210 kcal")?)
    );
    assert_eq!(
        Energy::parse("960 kJ")?,
        Energy::Kilojoules(Kilojoules::parse("960 kJ")?)
    );
    assert_eq!(Mass::parse("150 mg")?.value(), Decimal::from(150));
    assert_eq!(
        Mass::parse("< 0.5 g")?.qualifier(),
        Some(Qualifier::LessThan)
    );
    Ok(())
}

#[test]
fn keeps_float_revealing_decimals_exact() -> TestResult {
    // These values differ from their binary floating-point representation;
    // a float intermediate would corrupt the round trip.
    assert_eq!(Grams::parse("0.1 g")?.to_string(), "0.1 g");
    assert_eq!(Milligrams::parse("0.3 mg")?.to_string(), "0.3 mg");
    assert_eq!(Micrograms::parse("2.675 µg")?.to_string(), "2.675 µg");
    assert_eq!(Grams::parse("8.4 g")?.to_string(), "8.4 g");
    // Trailing zeros are the source's spelling and stay preserved.
    let reported = Grams::parse("1.00 g")?;
    assert_eq!(reported.to_string(), "1.00 g");
    let zero = Grams::parse("0.0 g")?;
    assert_eq!(zero.to_string(), "0.0 g");
    Ok(())
}

#[test]
fn keeps_negative_reported_amounts() -> TestResult {
    // USDA publishes negative carbohydrate-by-difference values for some
    // meats; the amount is kept as reported instead of being clamped.
    let negative = Grams::parse("-0.475 g")?;
    assert_eq!(negative.to_string(), "-0.475 g");
    Ok(())
}

#[test]
fn parses_scientific_notation_amounts() -> TestResult {
    // Foundation archives spell some sodium and cholesterol amounts this way.
    let uppercase = Milligrams::parse("4.10E+3 mg")?;
    assert_eq!(uppercase.value(), Decimal::from(4100));
    assert_eq!(uppercase.to_string(), "4100 mg");

    let lowercase = Milligrams::parse("5e-2 mg")?;
    assert_eq!(lowercase.to_string(), "0.05 mg");
    Ok(())
}

#[test]
fn scientific_notation_stays_exact_or_fails() -> TestResult {
    // A 29-fraction-digit coefficient with a +1 exponent shifts to a
    // representable value and must keep every digit.
    let shifted = Micrograms::parse("0.12345678901234567890123456789e1 µg")?;
    assert_eq!(shifted.to_string(), "1.2345678901234567890123456789 µg");

    // The same coefficient without the shift is unrepresentable and must be
    // rejected instead of silently rounded to 28 fractional digits.
    let unrepresentable = Micrograms::parse("0.12345678901234567890123456789e0 µg");
    expect_amount_error(unrepresentable)?;

    // Extreme exponents are rejected before any string is built.
    for text in [
        "1e+999999999 g",
        "1e-999999999 g",
        "1e29 g",
        "1e-29 g",
        "1e g",
        "1e2.5 g",
    ] {
        let rejected = Grams::parse(text);
        assert!(rejected.is_err(), "{text} was accepted");
    }
    Ok(())
}

#[test]
fn accepts_microgram_spellings() -> TestResult {
    // The micro sign, the Greek mu, and ASCII spellings all mean micrograms
    // and serialize to the canonical micro-sign spelling.
    for text in ["0.55 µg", "0.55 μg", "0.55 ug"] {
        let parsed = Micrograms::parse(text)?;
        assert_eq!(
            parsed.to_string(),
            "0.55 µg",
            "{text:?} did not canonicalize"
        );
    }
    Ok(())
}

#[test]
fn rejects_malformed_bearer_strings() -> TestResult {
    for text in ["15", "g", "15 g extra", "", "  ", "15 15", "~15 mg"] {
        assert!(
            Grams::parse(text).is_err(),
            "{text:?} was accepted as grams"
        );
    }
    Ok(())
}

#[test]
fn rejects_unknown_units() -> TestResult {
    expect_unsupported_unit(Mass::parse("15 floz"), "floz")?;
    expect_unsupported_unit(Mass::parse("15 kg"), "kg")?;
    expect_unsupported_unit(Mass::parse("15 cl"), "cl")?;
    expect_unsupported_unit(Mass::parse("15 oz"), "oz")?;
    Ok(())
}

#[test]
fn rejects_units_of_other_dimensions() -> TestResult {
    // Mass and energy units cannot be mixed.
    expect_unsupported_unit(Grams::parse("960 kJ"), "kJ")?;
    expect_unsupported_unit(Mass::parse("960 kJ"), "kJ")?;
    expect_unsupported_unit(Energy::parse("150 mg"), "mg")?;
    expect_unsupported_unit(Kilocalories::parse("210 g"), "g")?;
    expect_unsupported_unit(MeasuredVolume::parse("30 g"), "g")?;
    expect_unsupported_unit(MeasuredMass::parse("250 ml"), "ml")?;
    Ok(())
}

#[test]
fn rejects_unrepresentable_amounts() -> TestResult {
    // Twenty-nine fractional digits exceed the fixed-point scale.
    let too_precise = Grams::parse("0.12345678901234567890123456789 g");
    expect_amount_error(too_precise)?;

    // A 40-digit integer exceeds the decimal mantissa.
    let too_large = Grams::parse("1e+40 g");
    expect_amount_error(too_large)?;

    let also_too_precise = Grams::parse("5e-29 µg");
    expect_amount_error(also_too_precise)?;
    Ok(())
}

#[test]
fn parses_json_numbers_without_float_intermediates() -> TestResult {
    for (json, expected) in [
        ("0.1", "0.1"),
        ("1.00", "1.00"),
        ("28", "28"),
        ("4.10E+3", "4100"),
        ("0.5500", "0.5500"),
    ] {
        let number = serde_json::from_str::<JsonNumber>(json)?;
        let decimal = decimal_from_json_number(&number)?;
        assert_eq!(decimal.to_string(), expected, "{json} lost its spelling");
    }

    let too_precise = serde_json::from_str::<JsonNumber>("0.12345678901234567890123456789")?;
    let rejected = decimal_from_json_number(&too_precise);
    expect_amount_error(rejected)?;
    Ok(())
}

#[test]
fn rounds_measured_quantities_beyond_the_fixed_point_precision() -> TestResult {
    // USDA's Survey archive serializes some gram weights from binary floating
    // point; the documented policy rounds such spellings half to even to two
    // decimal places instead of dropping the serving.
    let artifact =
        serde_json::from_str::<JsonNumber>("9.199999999999999289457264239899814128875732421875")?;
    assert_eq!(
        measured_decimal_from_json_number(&artifact)?.to_string(),
        "9.2"
    );

    // Representable values keep their exact spelling and scale.
    for (json, expected) in [("246", "246"), ("30.8", "30.8"), ("0.5500", "0.5500")] {
        let number = serde_json::from_str::<JsonNumber>(json)?;
        assert_eq!(
            measured_decimal_from_json_number(&number)?.to_string(),
            expected,
            "{json} was rounded although it fits"
        );
    }

    let garbage = serde_json::from_str::<JsonNumber>("1e+40")?;
    let rejected = measured_decimal_from_json_number(&garbage);
    expect_amount_error(rejected)?;
    Ok(())
}

#[test]
fn builds_amounts_from_unit_names_and_values() -> TestResult {
    let sodium = Mass::from_parts("mg", Decimal::from(150), None)?;
    assert_eq!(sodium.to_string(), "150 mg");

    let grams = Mass::from_parts("g", Decimal::from_str_exact("0.0428")?, None)?;
    assert_eq!(grams.to_string(), "0.0428 g");

    let qualified = Mass::from_parts("g", Decimal::from(5), Some(Qualifier::Approximately))?;
    assert_eq!(qualified.to_string(), "~ 5 g");

    let energy = Energy::from_parts("kJ", Decimal::from(960), None)?;
    assert_eq!(energy.to_string(), "960 kJ");

    expect_unsupported_unit(Mass::from_parts("kJ", Decimal::from(1), None), "kJ")?;
    expect_unsupported_unit(Energy::from_parts("g", Decimal::from(1), None), "g")?;
    Ok(())
}

#[test]
fn parses_measured_quantities() -> TestResult {
    let mass = MeasuredMass::parse("30 g")?;
    assert_eq!(mass.unit(), MassUnit::Gram);
    assert_eq!(mass.value(), Decimal::from(30));
    assert_eq!(mass.to_string(), "30 g");

    let volume = MeasuredVolume::parse("250 ml")?;
    assert_eq!(volume.value(), Decimal::from(250));
    assert_eq!(volume.to_string(), "250 ml");

    let quantity = MeasuredQuantity::parse("0.5 mg")?;
    assert_eq!(quantity.to_string(), "0.5 mg");
    assert_eq!(quantity.value(), Decimal::from_str_exact("0.5")?);

    let built = MeasuredQuantity::from_parts("g", Decimal::from_str_exact("28.0")?)?;
    assert_eq!(built.to_string(), "28.0 g");

    expect_unsupported_unit(
        MeasuredQuantity::from_parts("kcal", Decimal::from(30)),
        "kcal",
    )?;
    Ok(())
}

#[test]
fn rejects_nonpositive_measured_quantities() -> TestResult {
    for text in ["0 g", "0.0 g", "-5 g", "0 ml"] {
        match MeasuredQuantity::parse(text) {
            Err(MeasureError::NotPositive { found }) => {
                assert!(
                    found <= Decimal::ZERO,
                    "{text:?} reported a positive amount"
                );
            }
            Err(other) => return Err(format!("unexpected error for {text:?}: {other}").into()),
            Ok(_) => return Err(format!("{text:?} was accepted as measured").into()),
        }
    }
    Ok(())
}

#[test]
fn rejects_qualified_measured_quantities() -> TestResult {
    for text in ["< 30 g", "~ 250 ml", "≥ 1 g"] {
        match MeasuredQuantity::parse(text) {
            Err(MeasureError::Qualified { .. }) => {}
            Err(other) => return Err(format!("unexpected error for {text:?}: {other}").into()),
            Ok(_) => return Err(format!("{text:?} was accepted as measured").into()),
        }
    }
    Ok(())
}

#[test]
fn serializes_amounts_as_bearer_strings() -> TestResult {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Wrapper {
        energy: Energy,
        sodium: Mass,
        serving: MeasuredQuantity,
    }

    let wrapper = Wrapper {
        energy: Energy::parse("~ 960 kJ")?,
        sodium: Mass::parse("150 mg")?,
        serving: MeasuredQuantity::parse("30 g")?,
    };
    let json = serde_json::to_string(&wrapper)?;
    assert_eq!(
        json,
        r#"{"energy":"~ 960 kJ","sodium":"150 mg","serving":"30 g"}"#
    );

    let round_tripped: Wrapper = serde_json::from_str(&json)?;
    assert_eq!(round_tripped.energy, wrapper.energy);
    assert_eq!(round_tripped.sodium, wrapper.sodium);
    assert_eq!(round_tripped.serving, wrapper.serving);
    Ok(())
}

#[test]
fn rejects_deserialized_garbage() -> TestResult {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        sodium: Mass,
    }

    let valid = serde_json::from_str::<Wrapper>(r#"{"sodium":"150 mg"}"#)?;
    assert_eq!(valid.sodium, Mass::parse("150 mg")?);

    let error = serde_json::from_str::<Wrapper>(r#"{"sodium":"150 floz"}"#);
    assert!(error.is_err(), "unknown unit was accepted in JSON");
    let error = serde_json::from_str::<Wrapper>(r#"{"sodium":150}"#);
    assert!(error.is_err(), "bare number was accepted in JSON");
    Ok(())
}

fn expect_unsupported_unit<T>(result: ParseResult<T>, unit: &str) -> Result<(), Box<dyn StdError>> {
    match result {
        Err(MeasureError::UnsupportedUnit { unit: found }) => {
            assert_eq!(found, unit, "wrong unit in error");
            Ok(())
        }
        Err(other) => Err(format!("expected unsupported unit {unit:?}, found: {other}").into()),
        Ok(_) => Err(format!("amount with unit {unit:?} was accepted").into()),
    }
}

fn expect_amount_error<T>(result: ParseResult<T>) -> Result<(), Box<dyn StdError>> {
    match result {
        Err(MeasureError::Amount { source }) => {
            let _ = source;
            Ok(())
        }
        Err(other) => Err(format!("expected an amount error, found: {other}").into()),
        Ok(_) => Err("unrepresentable amount was accepted".into()),
    }
}
