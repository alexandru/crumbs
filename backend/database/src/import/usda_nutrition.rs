//! Maps standardized USDA values without recalculating label or serving data.

use std::collections::BTreeMap;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::value::RawValue;

use super::FoodKind;
use crate::measurements::{Energy, Mass, MeasurementError, Reported, ServingQuantity, SignedMass};
use crate::nutrition::{FoodData, NutritionBasis, NutritionProfile, ServingSize};

#[derive(Debug, thiserror::Error)]
pub(crate) enum NutritionError {
    #[error("malformed USDA nutrient or serving fields: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid nutrient {id}: {source}")]
    Nutrient {
        id: NutrientId,
        #[source]
        source: MeasurementError,
    },
    #[error("nutrient {0} has an amount without a measurement unit")]
    MissingUnit(NutrientId),
    #[error("conflicting supplied values for nutrient {0}")]
    ConflictingNutrient(NutrientId),
    #[error("branded nutrients require an established gram or millilitre basis")]
    UnknownBasis,
    #[error("invalid serving quantity: {0}")]
    Serving(#[source] MeasurementError),
    #[error("measured serving has no name")]
    UnnamedServing,
    #[error("invalid energy unit for USDA nutrient {0}")]
    EnergyUnit(NutrientId),
    #[error("invalid limit of quantification for nutrient {0}")]
    InvalidLimit(NutrientId),
}

pub(crate) fn parse(raw: &str, kind: FoodKind) -> Result<FoodData, NutritionError> {
    let record: Record = serde_json::from_str(raw)?;
    let mut nutrients = BTreeMap::new();
    for nutrient in record.food_nutrients.iter().flatten() {
        let Some(id) = nutrient.nutrient.as_ref().and_then(|nutrient| nutrient.id) else {
            continue;
        };
        if !SUPPORTED_NUTRIENTS.contains(&id) {
            continue;
        }
        if let Some(previous) = nutrients.insert(id, nutrient) {
            // Duplicate observations cannot be collapsed if their values differ.
            if previous.amount.as_ref().map(|value| value.get())
                != nutrient.amount.as_ref().map(|value| value.get())
                || previous
                    .nutrient
                    .as_ref()
                    .and_then(|value| value.unit_name.as_deref())
                    != nutrient
                        .nutrient
                        .as_ref()
                        .and_then(|value| value.unit_name.as_deref())
                || previous.loq.as_ref().map(|value| value.get())
                    != nutrient.loq.as_ref().map(|value| value.get())
            {
                return Err(NutritionError::ConflictingNutrient(id));
            }
        }
    }

    let mut profile = NutritionProfile::new(NutritionBasis::Per100Grams);
    let mut energy = BTreeMap::new();
    let mut any_amount = false;
    for (id, nutrient) in nutrients {
        let Some(value) = amount_text(id, nutrient)? else {
            continue;
        };
        any_amount = true;
        if ENERGY_IDS.contains(&id) {
            let amount = Reported::<Energy>::from_str(&value)
                .map_err(|source| NutritionError::Nutrient { id, source })?;
            let valid_unit = matches!(
                (id, &amount.amount),
                (1062, Energy::Kilojoules(_)) | (2048 | 2047 | 1008, Energy::Kilocalories(_))
            );
            if !valid_unit {
                return Err(NutritionError::EnergyUnit(id));
            }
            energy.insert(id, amount);
        } else if id == 1005 {
            // USDA publishes signed estimates for carbohydrate by difference.
            profile.carbohydrates.including_fibre = Some(
                Reported::<SignedMass>::from_str(&value)
                    .map_err(|source| NutritionError::Nutrient { id, source })?,
            );
        } else {
            let amount = Reported::<Mass>::from_str(&value)
                .map_err(|source| NutritionError::Nutrient { id, source })?;
            let field = match id {
                1003 => &mut profile.protein,
                1004 => &mut profile.fat.total,
                1009 => &mut profile.carbohydrates.starch,
                1013 => &mut profile.carbohydrates.sugars.lactose,
                1063 | 2000 => &mut profile.carbohydrates.sugars.total,
                1079 => &mut profile.fibre,
                1093 => &mut profile.sodium,
                1235 => &mut profile.carbohydrates.sugars.added,
                1253 => &mut profile.cholesterol,
                1257 => &mut profile.fat.trans,
                1258 => &mut profile.fat.saturated,
                1292 => &mut profile.fat.monounsaturated,
                1293 => &mut profile.fat.polyunsaturated,
                _ => continue,
            };
            // USDA's newer total-sugars definition (2000) wins over 1063.
            // Both were parsed first, so invalid supplied values are reported.
            *field = Some(amount);
        }
    }
    profile.energy = ENERGY_IDS.iter().find_map(|id| energy.remove(id));
    let nutrition = if any_amount {
        profile.per = basis(kind, record.serving_size_unit.as_deref())?;
        Some(profile)
    } else {
        None
    };
    let mut serving_sizes = Vec::new();
    for portion in record.food_portions.into_iter().flatten() {
        let Some(weight) = portion.gram_weight.as_deref() else {
            continue;
        };
        let quantity = format!("{} g", number_text(weight)?)
            .parse()
            .map_err(NutritionError::Serving)?;
        let name = portion_name(&portion)?;
        serving_sizes.push(ServingSize { name, quantity });
    }
    if kind == FoodKind::Branded
        && let (Some(size), Some(unit)) = (
            record.serving_size.as_deref(),
            record.serving_size_unit.as_deref(),
        )
        && let Some(unit) = serving_unit(unit)
    {
        let quantity: ServingQuantity = format!("{} {unit}", number_text(size)?)
            .parse()
            .map_err(NutritionError::Serving)?;
        let name = record
            .household_serving_full_text
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| quantity.to_string());
        serving_sizes.push(ServingSize { name, quantity });
    }
    Ok(FoodData {
        nutrition,
        serving_sizes,
    })
}

type NutrientId = u32;

const ENERGY_IDS: [NutrientId; 4] = [2048, 2047, 1008, 1062];
const SUPPORTED_NUTRIENTS: [NutrientId; 19] = [
    1003, 1004, 1005, 1008, 1009, 1013, 1062, 1063, 1079, 1093, 1235, 1253, 1257, 1258, 1292, 1293,
    2000, 2047, 2048,
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    food_nutrients: Option<Vec<FoodNutrient>>,
    food_portions: Option<Vec<Portion>>,
    serving_size: Option<Box<RawValue>>,
    serving_size_unit: Option<String>,
    household_serving_full_text: Option<String>,
}

#[derive(Deserialize)]
struct FoodNutrient {
    nutrient: Option<Nutrient>,
    amount: Option<Box<RawValue>>,
    loq: Option<Box<RawValue>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Nutrient {
    id: Option<NutrientId>,
    unit_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Portion {
    gram_weight: Option<Box<RawValue>>,
    amount: Option<Box<RawValue>>,
    modifier: Option<String>,
    portion_description: Option<String>,
    measure_unit: Option<MeasureUnit>,
}

#[derive(Deserialize)]
struct MeasureUnit {
    name: Option<String>,
}

fn amount_text(id: NutrientId, nutrient: &FoodNutrient) -> Result<Option<String>, NutritionError> {
    let unit = nutrient
        .nutrient
        .as_ref()
        .and_then(|value| value.unit_name.as_deref());
    if let Some(limit) = &nutrient.loq {
        let limit = number_text(limit)?;
        let limit_decimal = crate::measurements::parse_decimal(&limit)
            .map_err(|source| NutritionError::Nutrient { id, source })?;
        if limit_decimal < Decimal::ZERO {
            return Err(NutritionError::InvalidLimit(id));
        }
        if limit_decimal > Decimal::ZERO {
            // USDA stores a below-LOQ component as zero plus its numerical LOQ.
            if let Some(amount) = &nutrient.amount {
                let amount = number_text(amount)?;
                if crate::measurements::parse_decimal(&amount)
                    .map_err(|source| NutritionError::Nutrient { id, source })?
                    != Decimal::ZERO
                {
                    return Err(NutritionError::InvalidLimit(id));
                }
            }
            let unit = unit.ok_or(NutritionError::MissingUnit(id))?;
            return Ok(Some(format!("< {limit} {}", canonical_unit(unit))));
        }
    }
    let Some(amount) = &nutrient.amount else {
        return Ok(None);
    };
    let unit = unit.ok_or(NutritionError::MissingUnit(id))?;
    Ok(Some(format!(
        "{} {}",
        number_text(amount)?,
        canonical_unit(unit)
    )))
}

fn number_text(raw: &RawValue) -> Result<String, serde_json::Error> {
    if raw.get().starts_with('"') {
        serde_json::from_str(raw.get())
    } else {
        // RawValue keeps decimal tokens away from serde's binary float visitor.
        Ok(raw.get().to_owned())
    }
}

fn canonical_unit(unit: &str) -> &str {
    match unit {
        "G" => "g",
        "MG" => "mg",
        "UG" => "µg",
        "KCAL" => "kcal",
        "KJ" => "kJ",
        other => other,
    }
}

fn basis(
    kind: FoodKind,
    serving_size_unit: Option<&str>,
) -> Result<NutritionBasis, NutritionError> {
    if kind != FoodKind::Branded {
        return Ok(NutritionBasis::Per100Grams);
    }
    // GBFPD standardizes foodNutrients to 100 units of the submitted serving
    // dimension. labelNutrients remain per serving and are not used here.
    // https://fdc.nal.usda.gov/GBFPD_Documentation/
    match serving_size_unit.map(str::trim) {
        Some("g" | "G" | "GRM") => Ok(NutritionBasis::Per100Grams),
        Some("ml" | "mL" | "ML" | "MLT") => Ok(NutritionBasis::Per100Millilitres),
        _ => Err(NutritionError::UnknownBasis),
    }
}

fn serving_unit(unit: &str) -> Option<&'static str> {
    match unit.trim() {
        "g" | "G" | "GRM" => Some("g"),
        "ml" | "mL" | "ML" | "MLT" => Some("ml"),
        _ => None,
    }
}

fn portion_name(portion: &Portion) -> Result<String, NutritionError> {
    if let Some(description) = &portion.portion_description
        && !description.trim().is_empty()
    {
        return Ok(description.clone());
    }
    let measure = portion
        .measure_unit
        .as_ref()
        .and_then(|unit| unit.name.as_deref())
        .filter(|name| !name.trim().is_empty() && *name != "undetermined");
    let modifier = portion
        .modifier
        .as_deref()
        .filter(|name| !name.trim().is_empty());
    let label = match (measure, modifier) {
        (Some(measure), Some(modifier)) => format!("{measure}, {modifier}"),
        (Some(label), None) | (None, Some(label)) => label.to_owned(),
        (None, None) => return Err(NutritionError::UnnamedServing),
    };
    if let Some(amount) = &portion.amount {
        let amount = number_text(amount)?;
        let amount =
            crate::measurements::parse_decimal(&amount).map_err(NutritionError::Serving)?;
        if amount <= Decimal::ZERO {
            return Err(NutritionError::UnnamedServing);
        }
        Ok(format!("{amount} {label}"))
    } else {
        Ok(label)
    }
}

#[cfg(test)]
#[path = "../../unit-tests/import/usda_nutrition.rs"]
mod tests;
