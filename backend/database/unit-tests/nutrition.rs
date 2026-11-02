use std::error::Error;

use super::*;
use crate::measurements::{Energy, Mass, Reported, ServingQuantity};

// Representative label inputs from Open Food Facts' documented nutrition
// fields: carbohydrates exclude fibre, salt is independent of sodium, and
// nutrient modifiers qualify the supplied value. This is a model compatibility
// fixture, not an Open Food Facts adapter.
// https://openfoodfacts.github.io/documentation/docs/Product-Opener/schemas/schemas/product_nutrition/
// https://openfoodfacts.github.io/openfoodfacts-server/dev/ref-perl-pod/ProductOpener/Nutrition.html
#[test]
fn european_label_fits_without_invented_nutrients() -> Result<(), Box<dyn Error>> {
    let input = r#"{
        "nutrition": {
            "per": "100 g",
            "energy": "~ 910 kJ",
            "fat": {"total":"8.4 g", "saturated":"1.2 g"},
            "carbohydrates": {
                "excluding_fibre":"28.5 g",
                "sugars":{"total":"4.2 g","added":"< 0.1 g","lactose":"0.55 g"}
            },
            "fibre":"4.0 g", "protein":"7.0 g", "salt":"<= 15 mg"
        },
        "serving_sizes":[
            {"name":"one slice","quantity":"30 g"},
            {"name":"two slices","quantity":"60 g"}
        ]
    }"#;
    let food: FoodData = serde_json::from_str(input)?;
    let profile = food.nutrition.as_ref().ok_or("missing profile")?;
    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(
        profile.energy,
        Some("~ 910 kJ".parse::<Reported<Energy>>()?)
    );
    assert_eq!(profile.carbohydrates.including_fibre, None);
    assert_eq!(
        profile.carbohydrates.excluding_fibre,
        Some("28.5 g".parse()?)
    );
    assert_eq!(profile.sodium, None);
    assert_eq!(profile.salt, Some("<= 15 mg".parse::<Reported<Mass>>()?));
    assert_eq!(food.serving_sizes.len(), 2);
    let encoded = serde_json::to_string(&food)?;
    assert_eq!(serde_json::from_str::<FoodData>(&encoded)?, food);
    assert!(!encoded.contains("usda"));
    assert!(!encoded.contains("openfoodfacts"));
    Ok(())
}

#[test]
fn volume_label_keeps_basis_and_servings_distinct_from_mass() -> Result<(), Box<dyn Error>> {
    let food: FoodData = serde_json::from_str(
        r#"{
        "nutrition":{"per":"100 ml","carbohydrates":{"excluding_fibre":"0 g"},
            "protein":"0.10000000000000001 g"},
        "serving_sizes":[{"name":"glass","quantity":"250 ml"}]
    }"#,
    )?;
    let profile = food.nutrition.as_ref().ok_or("missing profile")?;
    assert_eq!(profile.per, NutritionBasis::Per100Millilitres);
    assert_eq!(profile.carbohydrates.excluding_fibre, Some("0 g".parse()?));
    assert_eq!(profile.carbohydrates.sugars.lactose, None);
    assert_eq!(profile.protein, Some("0.10000000000000001 g".parse()?));
    assert_eq!(
        food.serving_sizes.first().map(|serving| &serving.quantity),
        Some(&"250 ml".parse::<ServingQuantity>()?)
    );
    assert_eq!(
        serde_json::from_str::<FoodData>(&serde_json::to_string(&food)?)?,
        food
    );
    Ok(())
}

#[test]
fn absent_nutrition_can_keep_measured_servings() -> Result<(), Box<dyn Error>> {
    let food: FoodData = serde_json::from_str(
        r#"{
        "nutrition":null,
        "serving_sizes":[{"name":"bottle","quantity":"1 l"}]
    }"#,
    )?;
    assert_eq!(food.nutrition, None);
    assert_eq!(food.serving_sizes.len(), 1);
    assert_eq!(
        serde_json::from_str::<FoodData>(&serde_json::to_string(&food)?)?,
        food
    );
    Ok(())
}

#[test]
fn rejects_invalid_basis_dimensions_and_nonpositive_servings() {
    for input in [
        r#"{"nutrition":{"per":"1 serving"}}"#,
        r#"{"nutrition":{"per":"100 g","energy":"15 g"}}"#,
        r#"{"nutrition":{"per":"100 ml","protein":"15 kcal"}}"#,
        r#"{"nutrition":null,"serving_sizes":[{"name":"cup","quantity":"0 ml"}]}"#,
        r#"{"nutrition":null,"serving_sizes":[{"name":"cup","quantity":"< 15 g"}]}"#,
    ] {
        assert!(serde_json::from_str::<FoodData>(input).is_err(), "{input}");
    }
}
