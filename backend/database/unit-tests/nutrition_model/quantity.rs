use std::error::Error;
use std::str::FromStr;

use super::*;

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
fn profile_mass_and_height_units_use_fixed_point_values() -> Result<(), Box<dyn Error>> {
    let weight = Kilograms::from_str("70.25 kg")?;
    let height = Centimetres::from_str("172.5 cm")?;
    assert_eq!(weight.to_string(), "70.25 kg");
    assert_eq!(height.to_string(), "172.5 cm");
    assert_eq!(serde_json::to_string(&weight)?, "\"70.25 kg\"");
    assert_eq!(serde_json::to_string(&height)?, "\"172.5 cm\"");
    assert!(Kilograms::from_str("70 lb").is_err());
    assert!(Centimetres::from_str("172 mm").is_err());
    assert!(Kilograms::from_str("-1 kg").is_err());
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
