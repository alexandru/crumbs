//! Model tests for `parsed_data`, including the Open Food Facts
//! compatibility checks required by the shared-model issue.
//!
//! The Open Food Facts examples are representative values taken from the
//! official product-nutrition documentation and real products; they check that
//! the shared model can hold another source's data, not that an importer
//! exists for it.

use std::error::Error as StdError;

use super::{
    Carbohydrates, Fat, InvalidServingName, NutritionBasis, NutritionProfile, ParsedFoodData,
    ServingName, ServingSize, Sugars,
};
use crate::measure::{Energy, Mass, MeasuredQuantity, Qualifier};

type TestResult = Result<(), Box<dyn StdError>>;

const ISSUE_EXAMPLE: &str = r#"{"nutrition":{"per":"100 g","energy":"210 kcal","fat":{"total":"8.4 g","saturated":"1.2 g","monounsaturated":null,"polyunsaturated":null,"trans":"< 0.1 g"},"carbohydrates":{"including_fibre":"28.5 g","excluding_fibre":null,"sugars":{"total":"4.2 g","added":null,"lactose":"0.55 g"},"starch":null},"fibre":"4.0 g","protein":"7.0 g","sodium":"150 mg","salt":null,"cholesterol":null},"serving_sizes":[{"name":"one slice","quantity":"30 g"},{"name":"two slices","quantity":"60 g"}]}"#;

#[test]
fn serializes_the_specified_json_shape() -> TestResult {
    let parsed = ParsedFoodData {
        nutrition: Some(NutritionProfile {
            per: NutritionBasis::Per100Grams,
            energy: Some(Energy::parse("210 kcal")?),
            fat: Fat {
                total: Some(Mass::parse("8.4 g")?),
                saturated: Some(Mass::parse("1.2 g")?),
                monounsaturated: None,
                polyunsaturated: None,
                trans: Some(Mass::parse("< 0.1 g")?),
            },
            carbohydrates: Carbohydrates {
                including_fibre: Some(Mass::parse("28.5 g")?),
                excluding_fibre: None,
                sugars: Sugars {
                    total: Some(Mass::parse("4.2 g")?),
                    added: None,
                    lactose: Some(Mass::parse("0.55 g")?),
                },
                starch: None,
            },
            fibre: Some(Mass::parse("4.0 g")?),
            protein: Some(Mass::parse("7.0 g")?),
            sodium: Some(Mass::parse("150 mg")?),
            salt: None,
            cholesterol: None,
        }),
        serving_sizes: vec![
            serving("one slice", "30 g")?,
            serving("two slices", "60 g")?,
        ],
    };

    assert_eq!(serde_json::to_string(&parsed)?, ISSUE_EXAMPLE);

    let round_tripped: ParsedFoodData = serde_json::from_str(ISSUE_EXAMPLE)?;
    assert_eq!(round_tripped, parsed);
    Ok(())
}

#[test]
fn allows_absent_nutrition_with_servings() -> TestResult {
    let json = r#"{"nutrition":null,"serving_sizes":[{"name":"one slice","quantity":"30 g"}]}"#;
    let parsed: ParsedFoodData = serde_json::from_str(json)?;
    assert_eq!(parsed.nutrition, None);
    assert_eq!(parsed.serving_sizes, vec![serving("one slice", "30 g")?]);

    let empty: ParsedFoodData = serde_json::from_str(r#"{"nutrition":null,"serving_sizes":[]}"#)?;
    assert_eq!(empty, ParsedFoodData::default());
    assert_eq!(
        serde_json::to_string(&ParsedFoodData::default())?,
        r#"{"nutrition":null,"serving_sizes":[]}"#
    );
    Ok(())
}

#[test]
fn keeps_missing_amounts_distinct_from_reported_zero() -> TestResult {
    let partially_populated: ParsedFoodData = serde_json::from_str(
        r#"{"nutrition":{"per":"100 ml","energy":"42.4 kcal","fat":{"total":"0 g","saturated":null,"monounsaturated":null,"polyunsaturated":null,"trans":null},"carbohydrates":{"including_fibre":null,"excluding_fibre":"10.6 g","sugars":{"total":"0 g","added":null,"lactose":null},"starch":null},"fibre":null,"protein":null,"sodium":null,"salt":"0 g","cholesterol":null},"serving_sizes":[]}"#,
    )?;
    let profile = match partially_populated.nutrition {
        Some(profile) => profile,
        None => return Err("nutrition was dropped".into()),
    };
    assert_eq!(profile.per, NutritionBasis::Per100Millilitres);
    // A reported zero stays a value; absence stays absent.
    assert_eq!(profile.fat.total, Some(Mass::parse("0 g")?));
    assert_eq!(profile.fat.saturated, None);
    assert_eq!(profile.energy, Some(Energy::parse("42.4 kcal")?));
    assert_eq!(profile.salt, Some(Mass::parse("0 g")?));
    assert_eq!(
        profile.carbohydrates.sugars.total,
        Some(Mass::parse("0 g")?)
    );
    assert_eq!(
        profile.carbohydrates.excluding_fibre,
        Some(Mass::parse("10.6 g")?)
    );
    assert_eq!(profile.carbohydrates.including_fibre, None);
    assert_eq!(profile.cholesterol, None);
    Ok(())
}

#[test]
fn parses_and_rejects_nutrition_bases() -> TestResult {
    assert_eq!(NutritionBasis::parse("100 g")?, NutritionBasis::Per100Grams);
    assert_eq!(
        NutritionBasis::parse("100 ml")?,
        NutritionBasis::Per100Millilitres
    );
    assert!(
        NutritionBasis::parse("100g").is_err(),
        "unspaced basis was accepted"
    );
    assert!(
        NutritionBasis::parse("50 g").is_err(),
        "unsupported basis was accepted"
    );
    Ok(())
}

#[test]
fn rejects_blank_serving_names() -> TestResult {
    assert!(ServingName::new("  ").is_err());
    assert_eq!(ServingName::new(" one slice ")?.as_str(), "one slice");
    assert!(matches!(ServingName::new("\t"), Err(InvalidServingName)));

    let error = serde_json::from_str::<ServingSize>(r#"{"name":"  ","quantity":"30 g"}"#);
    assert!(error.is_err(), "blank serving name was accepted in JSON");
    Ok(())
}

#[test]
fn open_food_facts_solid_product_fits_the_model() -> TestResult {
    // Representative Nutella values from Open Food Facts. OFF's
    // `carbohydrates` excludes fibre, so it maps to `excluding_fibre`;
    // sodium arrives in grams and keeps its source unit.
    let parsed = ParsedFoodData {
        nutrition: Some(NutritionProfile {
            per: NutritionBasis::Per100Grams,
            energy: Some(Energy::parse("539 kcal")?),
            fat: Fat {
                total: Some(Mass::parse("30.9 g")?),
                saturated: Some(Mass::parse("10.6 g")?),
                monounsaturated: None,
                polyunsaturated: None,
                trans: None,
            },
            carbohydrates: Carbohydrates {
                including_fibre: None,
                excluding_fibre: Some(Mass::parse("57.5 g")?),
                sugars: Sugars {
                    total: Some(Mass::parse("56.3 g")?),
                    added: Some(Mass::parse("52.13 g")?),
                    lactose: None,
                },
                starch: None,
            },
            fibre: Some(Mass::parse("0 g")?),
            protein: Some(Mass::parse("6.3 g")?),
            sodium: Some(Mass::parse("0.0428 g")?),
            salt: Some(Mass::parse("0.107 g")?),
            cholesterol: None,
        }),
        serving_sizes: Vec::new(),
    };

    let json = serde_json::to_string(&parsed)?;
    let round_tripped: ParsedFoodData = serde_json::from_str(&json)?;
    assert_eq!(round_tripped, parsed);
    assert_eq!(round_tripped.nutrition, parsed.nutrition);
    Ok(())
}

#[test]
fn open_food_facts_qualified_values_fit_the_model() -> TestResult {
    // Representative Coca-Cola values: OFF modifiers become qualifiers, and
    // both a zero amount and an approximate qualifier can coexist.
    let parsed = ParsedFoodData {
        nutrition: Some(NutritionProfile {
            per: NutritionBasis::Per100Grams,
            energy: Some(Energy::parse("~ 42.4 kcal")?),
            fat: Fat {
                total: Some(Mass::parse("0 g")?),
                saturated: Some(Mass::parse("0 g")?),
                monounsaturated: None,
                polyunsaturated: None,
                trans: None,
            },
            carbohydrates: Carbohydrates {
                including_fibre: None,
                excluding_fibre: Some(Mass::parse("10.6 g")?),
                sugars: Sugars {
                    total: Some(Mass::parse("10.6 g")?),
                    added: Some(Mass::parse("~ 10.6 g")?),
                    lactose: None,
                },
                starch: None,
            },
            fibre: Some(Mass::parse("0 g")?),
            protein: Some(Mass::parse("0 g")?),
            sodium: Some(Mass::parse("~ 0 g")?),
            salt: Some(Mass::parse("0 g")?),
            cholesterol: None,
        }),
        serving_sizes: vec![ServingSize {
            name: ServingName::new("1 portion (330 ml)")?,
            quantity: MeasuredQuantity::parse("330 ml")?,
        }],
    };

    let profile = match parsed.nutrition.as_ref() {
        Some(profile) => profile,
        None => return Err("nutrition was dropped".into()),
    };
    assert_eq!(
        profile.energy.and_then(|energy| energy.qualifier()),
        Some(Qualifier::Approximately)
    );
    assert_eq!(
        profile.sodium.and_then(|sodium| sodium.qualifier()),
        Some(Qualifier::Approximately)
    );

    let json = serde_json::to_string(&parsed)?;
    let round_tripped: ParsedFoodData = serde_json::from_str(&json)?;
    assert_eq!(round_tripped, parsed);
    Ok(())
}

#[test]
fn open_food_facts_liquid_product_fits_the_model() -> TestResult {
    // Representative UHT milk values: a millilitre basis and a kilojoule
    // energy, both retained from the source.
    let parsed = ParsedFoodData {
        nutrition: Some(NutritionProfile {
            per: NutritionBasis::Per100Millilitres,
            energy: Some(Energy::parse("~ 243.6 kJ")?),
            fat: Fat {
                total: Some(Mass::parse("3 g")?),
                saturated: Some(Mass::parse("0 g")?),
                monounsaturated: None,
                polyunsaturated: None,
                trans: None,
            },
            carbohydrates: Carbohydrates {
                including_fibre: None,
                excluding_fibre: Some(Mass::parse("4.8 g")?),
                sugars: Sugars {
                    total: Some(Mass::parse("0 g")?),
                    added: Some(Mass::parse("~ 0 g")?),
                    lactose: None,
                },
                starch: None,
            },
            fibre: Some(Mass::parse("0 g")?),
            protein: Some(Mass::parse("3 g")?),
            sodium: Some(Mass::parse("0 g")?),
            salt: Some(Mass::parse("0 g")?),
            cholesterol: None,
        }),
        serving_sizes: vec![ServingSize {
            name: ServingName::new("900ml")?,
            quantity: MeasuredQuantity::parse("900 ml")?,
        }],
    };

    let profile = match parsed.nutrition.as_ref() {
        Some(profile) => profile,
        None => return Err("nutrition was dropped".into()),
    };
    assert_eq!(profile.per, NutritionBasis::Per100Millilitres);
    assert_eq!(profile.energy, Some(Energy::parse("~ 243.6 kJ")?));

    let json = serde_json::to_string(&parsed)?;
    let round_tripped: ParsedFoodData = serde_json::from_str(&json)?;
    assert_eq!(round_tripped, parsed);
    Ok(())
}

fn serving(name: &str, quantity: &str) -> Result<ServingSize, Box<dyn StdError>> {
    Ok(ServingSize {
        name: ServingName::new(name)?,
        quantity: MeasuredQuantity::parse(quantity)?,
    })
}
