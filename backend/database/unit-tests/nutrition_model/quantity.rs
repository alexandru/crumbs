use std::error::Error;
use std::str::FromStr;

use rust_decimal::Decimal;

use super::*;

#[test]
fn signed_energy_and_reported_values_enforce_distinct_invariants() -> Result<(), Box<dyn Error>> {
    let ten = Kcal::new(Decimal::from(10));
    let twenty = Kcal::new(Decimal::from(20));
    assert_eq!((ten - twenty).to_string(), "-10 kcal");
    assert_eq!((ten + twenty).to_string(), "30 kcal");
    let negative = Kcal::from_str("-10 kcal")?;
    assert_eq!(
        serde_json::from_str::<Kcal>(&serde_json::to_string(&negative)?)?,
        negative
    );
    let energy = Energy::from_str("-10 kcal")?;
    assert!(Reported::new(Qualifier::Exact, energy).is_err());
    assert!(Reported::<Energy>::from_str("-10 kcal").is_err());
    assert!(serde_json::from_str::<Reported<Energy>>("\"-10 kcal\"").is_err());
    assert!(Reported::<Energy>::from_str("-0 kcal").is_ok());
    Ok(())
}

#[test]
fn daily_energy_arithmetic_and_coefficients_use_typed_values() -> Result<(), Box<dyn Error>> {
    let ten = Kcal::new(Decimal::from(10));
    let twenty = Kcal::new(Decimal::from(20));
    let daily = PerDay::new(ten);
    assert_eq!((daily + twenty).amount(), Kcal::new(Decimal::from(30)));
    assert_eq!((daily - twenty).amount(), Kcal::new(Decimal::from(-10)));
    for value in [daily, PerDay::new(-ten)] {
        assert_eq!(
            serde_json::from_str::<PerDay<Kcal>>(&serde_json::to_string(&value)?)?,
            value
        );
    }
    assert_eq!(daily.to_string(), "10 kcal/day");
    assert_eq!(serde_json::to_string(&daily)?, "\"10 kcal/day\"");
    assert_eq!(PerDay::<Kcal>::from_str("-10 kcal/day")?, PerDay::new(-ten));
    assert!(PerDay::<Kcal>::from_str("10 kcal").is_err());
    assert!(PerDay::<Kcal>::from_str("10 g/day").is_err());
    assert!(serde_json::from_str::<PerDay<Kcal>>("\"10 kcal\"").is_err());
    assert_eq!(KcalPerKg::new(ten) * Kilograms::from_str("2 kg")?, twenty);
    assert_eq!(KcalPerCm::new(ten) * Centimetres::from_str("2 cm")?, twenty);
    Ok(())
}

#[test]
fn ratios_preserve_compound_units_and_multiply_by_denominator() -> Result<(), Box<dyn Error>> {
    let density = Ratio::<Grams, Millilitres>::from_str("-1.5 g/ml")?;
    assert_eq!(density.unit_symbol(), "g/ml");
    assert_eq!(density.to_string(), "-1.5 g/ml");
    assert_eq!(
        density * Millilitres::from_str("2 ml")?,
        Grams::from_str("-3.0 g")?
    );
    assert_eq!(serde_json::to_string(&density)?, "\"-1.5 g/ml\"");
    assert_eq!(
        serde_json::from_str::<Ratio<Grams, Millilitres>>("\"-1.5 g/ml\"")?,
        density
    );
    assert!(Ratio::<Grams, Millilitres>::from_str("1 g").is_err());
    assert!(Ratio::<Grams, Millilitres>::from_str("1 g/kg").is_err());
    assert!(serde_json::from_str::<Ratio<Grams, Millilitres>>("\"1 g\"").is_err());
    let coefficient: Ratio<Kcal, Kilograms> = KcalPerKg::from(10);
    assert_eq!(coefficient.to_string(), "10 kcal/kg");
    let height: Ratio<Kcal, Centimetres> = KcalPerCm::from(Decimal::new(625, 2));
    assert_eq!(height.to_string(), "6.25 kcal/cm");
    Ok(())
}

#[test]
fn numeric_conversions_keep_values_exact_for_units_and_ratios() {
    assert_eq!(Grams::from(-5_i64).to_string(), "-5 g");
    assert_eq!(Millilitres::from(u64::MAX).value(), Decimal::from(u64::MAX));
    let density = Ratio::<Grams, Millilitres>::from(-3_i32);
    assert_eq!(density.to_string(), "-3 g/ml");
    let maximum = Ratio::<Grams, Millilitres>::from(u64::MAX);
    assert_eq!(maximum.amount().value(), Decimal::from(u64::MAX));
    assert_eq!(
        Ratio::<Grams, Millilitres>::from(Decimal::new(125, 2)).to_string(),
        "1.25 g/ml"
    );
}

#[test]
fn measurement_units_support_signed_arithmetic() {
    macro_rules! assert_arithmetic {
        ($unit:ty) => {{
            let one = <$unit>::new(Decimal::ONE);
            let two = <$unit>::new(Decimal::from(2));
            assert_eq!(one + one, two);
            assert_eq!(one - two, -one);
        }};
    }
    let grams = PerDay::new(Grams::new(Decimal::from(3)));
    let one_gram = Grams::new(Decimal::ONE);
    assert_eq!((grams - one_gram).amount(), Grams::new(Decimal::from(2)));
    assert_eq!((grams + one_gram).amount(), Grams::new(Decimal::from(4)));
    assert_eq!(grams.to_string(), "3 g/day");
    assert_eq!(grams.unit_symbol(), "g/day");

    assert_arithmetic!(Grams);
    assert_arithmetic!(Millilitres);
    assert_arithmetic!(Kcal);
}

#[test]
fn measurement_unit_symbols_follow_concrete_and_dimension_units() -> Result<(), Box<dyn Error>> {
    assert_eq!(Grams::new(Decimal::ONE).unit_symbol(), "g");
    assert_eq!(Kcal::new(Decimal::ONE).unit_symbol(), "kcal");
    assert_eq!(Mass::from_str("2 mg")?.unit_symbol(), "mg");
    assert_eq!(Energy::from_str("2 kJ")?.unit_symbol(), "kJ");
    assert_eq!(
        PerDay::<Grams>::from_str("-3 g/day")?.to_string(),
        "-3 g/day"
    );
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
    assert_eq!(
        serde_json::from_str::<Grams>("\"-1 g\"")?.to_string(),
        "-1 g"
    );
    assert!(serde_json::from_str::<Kilojoules>("\"1 kcal\"").is_err());
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
fn quantity_builder_parses_values_and_units_without_combining_strings() -> Result<(), Box<dyn Error>>
{
    let reported = Reported::<Mass>::parse_with_unit("< 0.55", "µg")?;
    assert_eq!(reported.qualifier(), Qualifier::LessThan);
    assert_eq!(reported.amount().to_string(), "0.55 µg");
    let copied = reported;
    assert_eq!(copied.amount().to_string(), "0.55 µg");

    assert!(Reported::<Mass>::parse_with_unit("0", "g").is_ok());
    assert!(Reported::<Mass>::parse_with_unit("-0.1", "g").is_err());
    assert!(Reported::<Mass>::parse_with_unit("1", "kcal").is_err());
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
        serde_json::to_string(&serde_json::from_str::<Kcal>(cases[4].0)?)?,
        cases[4].1
    );
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<Kilojoules>(cases[5].0)?)?,
        cases[5].1
    );
    Ok(())
}

#[test]
fn profile_mass_and_height_units_use_fixed_point_values() -> Result<(), Box<dyn Error>> {
    let weight = Kilograms::from_str("70.25 kg")?;
    let height = Centimetres::from_str("172.5 cm")?;
    assert_eq!(weight.to_string(), "70.25 kg");
    assert_eq!(height.to_string(), "172.5 cm");
    assert_eq!(serde_json::to_string(&weight)?, "\"70.25 kg\"");
    assert_eq!(serde_json::to_string(&height)?, "\"172.5 cm\"");
    assert!(Kilograms::from_str("70 lb").is_err());
    assert!(Centimetres::from_str("172 mm").is_err());
    assert_eq!(Kilograms::from_str("-1 kg")?.to_string(), "-1 kg");
    Ok(())
}

#[test]
fn mass_volume_and_energy_conversions_use_checked_metric_factors() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        Mass::from_str("1.25 kg")?.to_grams()?.value(),
        Decimal::from(1250)
    );
    assert_eq!(
        Mass::from_str("1250 mg")?.to_grams()?.value(),
        Decimal::new(125, 2)
    );
    assert_eq!(
        Mass::from_str("250000 µg")?.to_grams()?.value(),
        Decimal::new(25, 2)
    );
    assert_eq!(
        Volume::from_str("1.25 l")?.to_millilitres()?.value(),
        Decimal::from(1250)
    );
    assert_eq!(
        Volume::from_str("250 ml")?.to_millilitres()?.value(),
        Decimal::from(250)
    );
    assert_eq!(
        Energy::from_str("4.184 kJ")?.to_kilocalories()?.to_string(),
        "1 kcal"
    );
    assert_eq!(
        Energy::from_str("2 kcal")?.to_kilocalories()?.to_string(),
        "2 kcal"
    );

    for input in ["1 kg", "1 l", "250 ml"] {
        let quantity = MeasuredQuantity::from_str(input)?;
        assert_eq!(quantity.to_string(), input);
        let encoded = serde_json::to_string(&quantity)?;
        assert_eq!(encoded, format!("\"{input}\""));
        assert_eq!(
            serde_json::from_str::<MeasuredQuantity>(&encoded)?,
            quantity
        );
    }
    let quantity = MeasuredQuantity::from_str("1 l")?;
    assert_eq!(
        quantity.volume(),
        Some(Volume::Litres(Litres::from_str("1 l")?))
    );
    assert!(MeasuredQuantity::from_str("1 oz").is_err());
    Ok(())
}

#[test]
fn checked_scaling_preserves_units_and_qualifiers_and_reports_rounding_overflow()
-> Result<(), Box<dyn Error>> {
    let mass = Mass::from_str("1.25 kg")?.checked_scale(Decimal::from(2))?;
    assert_eq!(mass.to_string(), "2.50 kg");
    let volume = Volume::from_str("2 l")?.checked_scale(Decimal::new(15, 1))?;
    assert_eq!(volume.to_string(), "3.0 l");
    let energy = Energy::from_str("2 kJ")?.checked_scale(Decimal::from(2))?;
    assert_eq!(energy.to_string(), "4 kJ");

    let reported = Reported::<Mass>::from_str("< 2 mg")?.checked_scale(Decimal::from(3))?;
    assert_eq!(reported.to_string(), "< 6 mg");
    let quantity = MeasuredQuantity::from_str("2 ml")?.checked_scale(Decimal::new(5, 1))?;
    assert_eq!(quantity.to_string(), "1.0 ml");

    let odd_tie = Grams::new(Decimal::new(3, 28)).checked_scale(Decimal::new(5, 1))?;
    assert_eq!(odd_tie.value(), Decimal::new(2, 28));
    let even_tie = Grams::new(Decimal::new(1, 28)).checked_scale(Decimal::new(5, 1))?;
    assert_eq!(even_tie.value(), Decimal::ZERO);
    let strict_upper = Reported::new(Qualifier::LessThan, Grams::new(Decimal::new(1, 28)))?;
    assert!(matches!(
        strict_upper.checked_scale(Decimal::new(5, 1)),
        Err(QuantityError::ZeroStrictUpperBound)
    ));
    assert!(matches!(
        Grams::new(Decimal::MAX).checked_scale(Decimal::from(2)),
        Err(QuantityError::Arithmetic)
    ));
    assert_eq!(
        Grams::new(Decimal::ONE)
            .checked_scale(Decimal::from(-1))?
            .value(),
        Decimal::NEGATIVE_ONE
    );
    assert!(matches!(
        Reported::new(Qualifier::Exact, Grams::new(Decimal::ONE))?.checked_scale(Decimal::from(-1)),
        Err(QuantityError::Negative)
    ));
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
