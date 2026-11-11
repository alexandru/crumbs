use std::error::Error;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde_json::json;

use super::*;

#[test]
fn food_data_round_trips_mass_and_volume_profiles_without_defaulting_missing_amounts()
-> Result<(), Box<dyn Error>> {
    let mass_fixture = json!({
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
    });
    let mass_data: FoodData = serde_json::from_value(mass_fixture.clone())?;
    let encoded = serde_json::to_value(&mass_data)?;
    let nutrition = &encoded["nutrition"];
    assert!(nutrition.get("nutrients").is_none());
    assert_eq!(nutrition["fat"]["monounsaturated"], serde_json::Value::Null);
    assert_eq!(
        nutrition["carbohydrates"]["starch"],
        serde_json::Value::Null
    );
    assert_eq!(nutrition["protein"], "7.0 g");
    let decoded: FoodData = serde_json::from_value(encoded)?;
    assert_eq!(decoded, mass_data);
    let profile = mass_data.nutrition.as_ref().ok_or("profile is missing")?;
    assert_eq!(profile.per, NutritionBasis::Per100Grams);
    assert_eq!(
        profile
            .nutrients
            .carbohydrates
            .including_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("28.5 g")
    );
    assert_eq!(profile.nutrients.carbohydrates.excluding_fibre, None);
    assert_eq!(
        profile
            .nutrients
            .carbohydrates
            .sugars
            .lactose
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("0.55 g")
    );
    assert_eq!(
        profile
            .nutrients
            .sodium
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("150 mg")
    );
    assert_eq!(profile.nutrients.salt, None);

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
    assert_eq!(volume_profile.nutrients.fat.total, None);
    assert_eq!(volume_profile.nutrients.carbohydrates.sugars.lactose, None);
    assert_eq!(volume_profile.nutrients.sodium, None);
    assert_eq!(volume_profile.nutrients.salt, None);
    Ok(())
}

#[test]
fn consumed_nutrition_has_required_calories_and_flattened_nutrients() -> Result<(), Box<dyn Error>>
{
    let input = json!({
        "calories": "120 kcal",
        "protein": "7 g",
        "fat": { "total": "2 g" },
        "carbohydrates": { "including_fibre": "18 g" }
    });
    let consumed: ConsumedNutrition = serde_json::from_value(input.clone())?;
    let encoded = serde_json::to_value(&consumed)?;
    assert_eq!(encoded["calories"], "120 kcal");
    assert_eq!(encoded["protein"], "7 g");
    assert_eq!(encoded["fibre"], serde_json::Value::Null);
    assert!(encoded.get("nutrients").is_none());
    assert!(serde_json::from_str::<ConsumedNutrition>(r#"{"protein":"7 g"}"#).is_err());
    assert!(serde_json::from_str::<ConsumedNutrition>(r#"{"calories":"4.184 kJ"}"#).is_err());
    assert_eq!(consumed.nutrients.fibre, None);
    Ok(())
}

#[test]
fn nutrient_scaling_preserves_qualifiers_units_and_missing_values() -> Result<(), Box<dyn Error>> {
    let nutrients: Nutrients = serde_json::from_value(json!({
        "fat": {
            "total": "<= 10 mg",
            "saturated": "2 g",
            "monounsaturated": "3 µg",
            "polyunsaturated": null,
            "trans": "~ 4 mg"
        },
        "carbohydrates": {
            "including_fibre": "20 g",
            "excluding_fibre": null,
            "sugars": { "total": "5 g", "added": "1 g", "lactose": null },
            "starch": "14 g"
        },
        "fibre": "2 g",
        "protein": "< 3 g",
        "sodium": "5 mg",
        "salt": null,
        "cholesterol": "6 µg"
    }))?;
    let scaled = nutrients.checked_scale(Decimal::new(5, 1))?;
    assert_eq!(
        scaled.fat.total.map(|value| value.to_string()).as_deref(),
        Some("<= 5.0 mg")
    );
    assert_eq!(
        scaled
            .fat
            .saturated
            .map(|value| value.to_string())
            .as_deref(),
        Some("1.0 g")
    );
    assert_eq!(
        scaled
            .fat
            .monounsaturated
            .map(|value| value.to_string())
            .as_deref(),
        Some("1.5 µg")
    );
    assert_eq!(scaled.fat.polyunsaturated, None);
    assert_eq!(
        scaled.fat.trans.map(|value| value.to_string()).as_deref(),
        Some("~ 2.0 mg")
    );
    assert_eq!(
        scaled
            .carbohydrates
            .including_fibre
            .map(|value| value.to_string())
            .as_deref(),
        Some("10.0 g")
    );
    assert_eq!(scaled.carbohydrates.excluding_fibre, None);
    assert_eq!(
        scaled
            .carbohydrates
            .sugars
            .total
            .map(|value| value.to_string())
            .as_deref(),
        Some("2.5 g")
    );
    assert_eq!(
        scaled
            .carbohydrates
            .sugars
            .added
            .map(|value| value.to_string())
            .as_deref(),
        Some("0.5 g")
    );
    assert_eq!(scaled.carbohydrates.sugars.lactose, None);
    assert_eq!(
        scaled
            .carbohydrates
            .starch
            .map(|value| value.to_string())
            .as_deref(),
        Some("7.0 g")
    );
    assert_eq!(
        scaled.fibre.map(|value| value.to_string()).as_deref(),
        Some("1.0 g")
    );
    assert_eq!(
        scaled.protein.map(|value| value.to_string()).as_deref(),
        Some("< 1.5 g")
    );
    assert_eq!(
        scaled.sodium.map(|value| value.to_string()).as_deref(),
        Some("2.5 mg")
    );
    assert_eq!(scaled.salt, None);
    assert_eq!(
        scaled.cholesterol.map(|value| value.to_string()).as_deref(),
        Some("3.0 µg")
    );
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
        profile
            .nutrients
            .protein
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("0 g")
    );
    assert_eq!(profile.nutrients.sodium, None);
    assert_eq!(
        profile
            .nutrients
            .salt
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("0.2 g")
    );
    assert_eq!(profile.nutrients.carbohydrates.including_fibre, None);
    assert_eq!(profile.nutrients.carbohydrates.excluding_fibre, None);
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
            .map(ServingSize::quantity)
            .and_then(MeasuredQuantity::volume)
            .is_none()
    );
    assert!(
        data.serving_sizes
            .get(2)
            .map(ServingSize::quantity)
            .and_then(MeasuredQuantity::mass)
            .is_none()
    );
    assert_eq!(
        data.serving_sizes
            .first()
            .map(|serving| serving.quantity().to_string())
            .as_deref(),
        Some("30 g")
    );
    assert_eq!(
        data.serving_sizes
            .get(2)
            .map(|serving| serving.quantity().to_string())
            .as_deref(),
        Some("240 ml")
    );
    assert_eq!(
        data.nutrition
            .as_ref()
            .ok_or("profile is missing")?
            .nutrients
            .protein
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("7 g")
    );
    let zero_mass = MeasuredQuantity::from_str("0 g")?;
    let zero_volume = MeasuredQuantity::from_str("0 ml")?;
    assert!(zero_mass.value().is_zero());
    assert!(zero_volume.value().is_zero());
    for input in ["1 kcal", "~ 1 g", "1 lb"] {
        assert!(MeasuredQuantity::from_str(input).is_err());
    }
    assert!(MeasuredQuantity::from_str("1 g").is_ok());
    assert!(MeasuredQuantity::from_str("1 ml").is_ok());
    assert!(ServingSize::new("empty".to_owned(), zero_mass).is_err());
    assert!(ServingSize::new("empty".to_owned(), zero_volume).is_err());
    assert!(serde_json::from_str::<ServingSize>(r#"{"name":"empty","quantity":"0 g"}"#).is_err());
    Ok(())
}

#[test]
fn measured_quantity_parses_zero_while_serving_size_rejects_it() -> Result<(), Box<dyn Error>> {
    let zero = MeasuredQuantity::from_str("0 g")?;
    assert!(zero.value().is_zero());
    assert!(ServingSize::new("empty".to_owned(), zero).is_err());
    assert_eq!(
        MeasuredQuantity::from_str("0.55 mg")?.value().to_string(),
        "0.55"
    );
    for quantity in ["1 g", "1 ml"] {
        let parsed = MeasuredQuantity::from_str(quantity)?;
        let encoded = serde_json::to_string(&parsed)?;
        let decoded: MeasuredQuantity = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, parsed);
    }
    assert!(serde_json::from_str::<MeasuredQuantity>("\"0 g\"").is_ok());
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
            .nutrients
            .carbohydrates
            .excluding_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("28.5 g")
    );
    assert_eq!(
        european_profile.nutrients.carbohydrates.including_fibre,
        None
    );
    let us_profile = us.nutrition.as_ref().ok_or("US profile is missing")?;
    assert_eq!(
        us_profile
            .nutrients
            .carbohydrates
            .including_fibre
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("28.5 g")
    );
    assert_eq!(us_profile.nutrients.carbohydrates.excluding_fibre, None);
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
            .nutrients
            .sodium
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("~ 150 mg")
    );
    assert_eq!(
        beverage_profile
            .nutrients
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
fn serving_sizes_reject_negative_mass_and_volume() -> Result<(), Box<dyn Error>> {
    for quantity in ["-1 g", "-1 ml"] {
        let measured = MeasuredQuantity::from_str(quantity)?;
        assert!(ServingSize::new("negative".to_owned(), measured).is_err());
        let input = json!({"name": "negative", "quantity": quantity});
        assert!(serde_json::from_value::<ServingSize>(input).is_err());
    }
    Ok(())
}
