use std::str::FromStr;

use serde::Deserialize;
use serde_json::value::RawValue;

use crate::nutrition::{
    Carbohydrates, Decimal, Energy, Fat, Mass, MeasurementError, NutritionBasis, NutritionProfile,
    ParsedFoodData, Reported, ServingQuantity, ServingSize,
};

use super::FoodKind;

pub(in crate::import) type NutrientId = u64;

/// Errors produced while converting USDA nutrition data into the shared model.
#[derive(Debug, thiserror::Error)]
pub(in crate::import) enum NutritionError {
    /// The USDA food record could not be deserialized.
    #[error("invalid USDA nutrition record: {source}")]
    Json {
        /// The serde error for the food record.
        #[from]
        source: serde_json::Error,
    },
    /// A supported nutrient amount or unit is invalid.
    #[error("invalid USDA nutrient {id}: {source}")]
    Nutrient {
        /// USDA nutrient identifier.
        id: NutrientId,
        /// The measurement parsing or normalization failure.
        #[source]
        source: MeasurementError,
    },
    /// More than one amount was supplied for the same supported nutrient.
    #[error("USDA nutrient {id} has duplicate amounts")]
    DuplicateNutrient {
        /// USDA nutrient identifier.
        id: NutrientId,
    },
    /// A declared serving quantity is invalid.
    #[error("invalid USDA serving quantity: {source}")]
    Serving {
        /// The measurement parsing failure.
        #[source]
        source: MeasurementError,
    },
    /// The standardized USDA branded nutrient basis cannot be established.
    #[error("cannot establish a 100 g or 100 ml basis for USDA branded nutrient values")]
    UnknownBrandedBasis,
}

/// Parses the supported USDA nutrition and serving fields from one food record.
pub(super) fn parse(raw: &str, kind: FoodKind) -> Result<ParsedFoodData, NutritionError> {
    let record: FoodRecord = serde_json::from_str(raw)?;
    let serving_sizes = match kind {
        FoodKind::Branded => parse_branded_serving(&record)?,
        FoodKind::Foundation | FoodKind::SrLegacy | FoodKind::Survey => {
            parse_food_portions(&record, kind)?
        }
    };
    let standardized =
        parse_standardized_nutrients(record.food_nutrients.as_deref().unwrap_or(&[]))?;

    // GBFPD documents 100 g or 100 ml values but no export discriminator; servingSizeUnit does
    // not establish the basis for foodNutrients: https://fdc.nal.usda.gov/GBFPD_Documentation/
    let nutrition = match kind {
        FoodKind::Foundation | FoodKind::SrLegacy | FoodKind::Survey => {
            standardized.into_profile(NutritionBasis::Per100Grams)
        }
        FoodKind::Branded => {
            let labels = parse_label_fallback(&record, serving_sizes.first())?;
            if !standardized.is_empty() && labels.is_none() {
                return Err(NutritionError::UnknownBrandedBasis);
            }
            labels
        }
    };

    Ok(ParsedFoodData {
        nutrition,
        serving_sizes,
    })
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct FoodRecord {
    #[serde(default)]
    food_nutrients: Option<Vec<FoodNutrient>>,
    #[serde(default)]
    food_portions: Option<Vec<FoodPortion>>,
    #[serde(default)]
    serving_size: Option<Box<RawValue>>,
    #[serde(default)]
    serving_size_unit: Option<String>,
    #[serde(default)]
    household_serving_full_text: Option<String>,
    #[serde(default)]
    label_nutrients: Option<LabelNutrients>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FoodNutrient {
    #[serde(default)]
    nutrient: Option<NutrientInfo>,
    #[serde(default)]
    amount: Option<Box<RawValue>>,
    #[serde(default)]
    loq: Option<Box<RawValue>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NutrientInfo {
    #[serde(default)]
    id: Option<NutrientId>,
    #[serde(default)]
    unit_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FoodPortion {
    #[serde(default)]
    amount: Option<Box<RawValue>>,
    #[serde(default)]
    measure_unit: Option<MeasureUnit>,
    #[serde(default)]
    modifier: Option<String>,
    #[serde(default)]
    gram_weight: Option<Box<RawValue>>,
    #[serde(default)]
    portion_description: Option<String>,
}

#[derive(Deserialize)]
struct MeasureUnit {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LabelNutrients {
    #[serde(default)]
    calories: Option<LabelValue>,
    #[serde(default)]
    fat: Option<LabelValue>,
    #[serde(default)]
    saturated_fat: Option<LabelValue>,
    #[serde(default)]
    trans_fat: Option<LabelValue>,
    #[serde(default)]
    carbohydrates: Option<LabelValue>,
    #[serde(default)]
    sugars: Option<LabelValue>,
    #[serde(default)]
    fiber: Option<LabelValue>,
    #[serde(default)]
    protein: Option<LabelValue>,
    #[serde(default)]
    sodium: Option<LabelValue>,
    #[serde(default)]
    cholesterol: Option<LabelValue>,
}

#[derive(Deserialize)]
struct LabelValue {
    #[serde(default)]
    value: Option<Box<RawValue>>,
}

#[derive(Default)]
struct NutritionValues {
    fat: Fat,
    carbohydrates: Carbohydrates,
    fibre: Option<Reported<Mass>>,
    protein: Option<Reported<Mass>>,
    sodium: Option<Reported<Mass>>,
    cholesterol: Option<Reported<Mass>>,
    energy: Option<Reported<Energy>>,
}

impl NutritionValues {
    fn is_empty(&self) -> bool {
        self.fat.total.is_none()
            && self.fat.saturated.is_none()
            && self.fat.monounsaturated.is_none()
            && self.fat.polyunsaturated.is_none()
            && self.fat.trans.is_none()
            && self.carbohydrates.including_fibre.is_none()
            && self.carbohydrates.excluding_fibre.is_none()
            && self.carbohydrates.sugars.total.is_none()
            && self.carbohydrates.sugars.added.is_none()
            && self.carbohydrates.sugars.lactose.is_none()
            && self.carbohydrates.starch.is_none()
            && self.fibre.is_none()
            && self.protein.is_none()
            && self.sodium.is_none()
            && self.cholesterol.is_none()
            && self.energy.is_none()
    }

    fn into_profile(self, basis: NutritionBasis) -> Option<NutritionProfile> {
        if self.is_empty() {
            return None;
        }

        let mut profile = NutritionProfile::new(basis);
        profile.fat = self.fat;
        profile.carbohydrates = self.carbohydrates;
        profile.fibre = self.fibre;
        profile.protein = self.protein;
        profile.sodium = self.sodium;
        profile.cholesterol = self.cholesterol;
        profile.energy = self.energy;
        Some(profile)
    }

    fn scale_exact(
        &mut self,
        numerator: Decimal,
        denominator: Decimal,
    ) -> Result<(), NutritionError> {
        scale_energy(&mut self.energy, 1008, numerator, denominator)?;
        scale_mass(&mut self.fat.total, 1004, numerator, denominator)?;
        scale_mass(&mut self.fat.saturated, 1258, numerator, denominator)?;
        scale_mass(&mut self.fat.trans, 1257, numerator, denominator)?;
        scale_mass(
            &mut self.carbohydrates.including_fibre,
            1005,
            numerator,
            denominator,
        )?;
        scale_mass(
            &mut self.carbohydrates.sugars.total,
            2000,
            numerator,
            denominator,
        )?;
        scale_mass(&mut self.fibre, 1079, numerator, denominator)?;
        scale_mass(&mut self.protein, 1003, numerator, denominator)?;
        scale_mass(&mut self.sodium, 1093, numerator, denominator)?;
        scale_mass(&mut self.cholesterol, 1253, numerator, denominator)?;
        Ok(())
    }
}

fn parse_standardized_nutrients(
    nutrients: &[FoodNutrient],
) -> Result<NutritionValues, NutritionError> {
    let mut values = NutritionValues::default();
    for nutrient in nutrients {
        let Some(id) = nutrient_id(nutrient) else {
            continue;
        };
        let Some(raw) = nutrient.amount.as_deref() else {
            continue;
        };
        let destination = match id {
            1003 => &mut values.protein,
            1004 => &mut values.fat.total,
            1005 => &mut values.carbohydrates.including_fibre,
            1009 => &mut values.carbohydrates.starch,
            1013 => &mut values.carbohydrates.sugars.lactose,
            1079 => &mut values.fibre,
            1093 => &mut values.sodium,
            1235 => &mut values.carbohydrates.sugars.added,
            1253 => &mut values.cholesterol,
            1257 => &mut values.fat.trans,
            1258 => &mut values.fat.saturated,
            1292 => &mut values.fat.monounsaturated,
            1293 => &mut values.fat.polyunsaturated,
            _ => continue,
        };
        if destination.is_some() {
            return Err(NutritionError::DuplicateNutrient { id });
        }
        let unit = nutrient
            .nutrient
            .as_ref()
            .and_then(|info| info.unit_name.as_deref())
            .unwrap_or("");
        let amount = parse_mass(raw, nutrient.loq.as_deref(), unit, id)?;
        *destination = Some(amount);
    }

    if let Some((id, nutrient, raw)) = preferred_nutrient(nutrients, &[2048, 2047, 1008, 1062])? {
        let unit = nutrient
            .nutrient
            .as_ref()
            .and_then(|info| info.unit_name.as_deref())
            .unwrap_or("");
        values.energy = Some(parse_energy(raw, nutrient.loq.as_deref(), unit, id)?);
    }

    // This adapter prefers ID 2000 and falls back to ID 1063 only when 2000 has no amount.
    // This is an adapter policy, not a USDA ranking or a claim of universal equivalence.
    if let Some((id, nutrient, raw)) = preferred_nutrient(nutrients, &[2000, 1063])? {
        let unit = nutrient
            .nutrient
            .as_ref()
            .and_then(|info| info.unit_name.as_deref())
            .unwrap_or("");
        values.carbohydrates.sugars.total =
            Some(parse_mass(raw, nutrient.loq.as_deref(), unit, id)?);
    }

    Ok(values)
}

fn parse_food_portions(
    record: &FoodRecord,
    kind: FoodKind,
) -> Result<Vec<ServingSize>, NutritionError> {
    let mut servings = Vec::new();
    for portion in record.food_portions.as_deref().unwrap_or(&[]) {
        let Some(raw_weight) = portion.gram_weight.as_deref() else {
            continue;
        };
        let weight = decimal_text(raw_weight).map_err(serving_error)?;
        let quantity = format!("{weight} g")
            .parse::<ServingQuantity>()
            .map_err(serving_error)?;
        let name = portion_name(portion, kind, &quantity)?;
        servings.push(ServingSize { name, quantity });
    }
    Ok(servings)
}

fn portion_name(
    portion: &FoodPortion,
    kind: FoodKind,
    quantity: &ServingQuantity,
) -> Result<String, NutritionError> {
    if matches!(kind, FoodKind::SrLegacy | FoodKind::Survey)
        && let Some(description) = portion
            .portion_description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    {
        return Ok(description.to_owned());
    }

    let mut parts = Vec::new();
    if let Some(amount) = portion.amount.as_deref() {
        parts.push(decimal_text(amount).map_err(serving_error)?);
    }
    if let Some(name) = portion
        .measure_unit
        .as_ref()
        .and_then(|unit| unit.name.as_deref())
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(name.to_owned());
    }
    if let Some(modifier) = portion
        .modifier
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(modifier.to_owned());
    }
    if parts.is_empty() {
        Ok(quantity.to_string())
    } else {
        Ok(parts.join(" "))
    }
}

fn parse_branded_serving(record: &FoodRecord) -> Result<Vec<ServingSize>, NutritionError> {
    let Some(raw_size) = record.serving_size.as_deref() else {
        return Ok(Vec::new());
    };
    let Some(unit) = record
        .serving_size_unit
        .as_deref()
        .and_then(branded_serving_unit)
    else {
        return Ok(Vec::new());
    };
    let amount = decimal_text(raw_size).map_err(serving_error)?;
    let quantity = format!("{amount} {unit}")
        .parse::<ServingQuantity>()
        .map_err(serving_error)?;
    let name = record
        .household_serving_full_text
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| quantity.to_string());
    Ok(vec![ServingSize { name, quantity }])
}

/// Normalizes per-serving label nutrients exactly to the corresponding 100 g or 100 ml profile basis.
/// Returns `PrecisionLoss` when the result is not representable at the supported precision.
fn parse_label_fallback(
    record: &FoodRecord,
    serving: Option<&ServingSize>,
) -> Result<Option<NutritionProfile>, NutritionError> {
    let (Some(labels), Some(serving)) = (record.label_nutrients.as_ref(), serving) else {
        return Ok(None);
    };
    let mut values = NutritionValues {
        energy: label_energy(labels.calories.as_ref(), 1008)?,
        fat: Fat {
            total: label_mass(labels.fat.as_ref(), "g", 1004)?,
            saturated: label_mass(labels.saturated_fat.as_ref(), "g", 1258)?,
            trans: label_mass(labels.trans_fat.as_ref(), "g", 1257)?,
            ..Fat::default()
        },
        carbohydrates: Carbohydrates {
            including_fibre: label_mass(labels.carbohydrates.as_ref(), "g", 1005)?,
            sugars: crate::nutrition::Sugars {
                total: label_mass(labels.sugars.as_ref(), "g", 2000)?,
                ..crate::nutrition::Sugars::default()
            },
            ..Carbohydrates::default()
        },
        fibre: label_mass(labels.fiber.as_ref(), "g", 1079)?,
        protein: label_mass(labels.protein.as_ref(), "g", 1003)?,
        sodium: label_mass(labels.sodium.as_ref(), "mg", 1093)?,
        cholesterol: label_mass(labels.cholesterol.as_ref(), "mg", 1253)?,
    };
    if values.is_empty() {
        return Ok(None);
    }
    let (numerator, denominator) = serving.quantity.hundred_ratio().map_err(serving_error)?;
    values.scale_exact(numerator, denominator)?;
    Ok(values.into_profile(serving.quantity.basis()))
}

fn label_mass(
    value: Option<&LabelValue>,
    unit: &str,
    id: NutrientId,
) -> Result<Option<Reported<Mass>>, NutritionError> {
    value
        .and_then(|label| label.value.as_deref())
        .map(|raw| parse_mass(raw, None, unit, id))
        .transpose()
}

fn label_energy(
    value: Option<&LabelValue>,
    id: NutrientId,
) -> Result<Option<Reported<Energy>>, NutritionError> {
    value
        .and_then(|label| label.value.as_deref())
        .map(|raw| parse_energy(raw, None, "kcal", id))
        .transpose()
}

fn scale_mass(
    amount: &mut Option<Reported<Mass>>,
    id: NutrientId,
    numerator: Decimal,
    denominator: Decimal,
) -> Result<(), NutritionError> {
    *amount = amount
        .as_ref()
        .map(|reported| {
            reported
                .checked_ratio(numerator, denominator)
                .map_err(|source| NutritionError::Nutrient { id, source })
        })
        .transpose()?;
    Ok(())
}

fn scale_energy(
    amount: &mut Option<Reported<Energy>>,
    id: NutrientId,
    numerator: Decimal,
    denominator: Decimal,
) -> Result<(), NutritionError> {
    *amount = amount
        .as_ref()
        .map(|reported| {
            reported
                .checked_ratio(numerator, denominator)
                .map_err(|source| NutritionError::Nutrient { id, source })
        })
        .transpose()?;
    Ok(())
}

fn parse_mass(
    raw: &RawValue,
    loq: Option<&RawValue>,
    unit: &str,
    id: NutrientId,
) -> Result<Reported<Mass>, NutritionError> {
    let measurement = amount_with_unit(raw, loq, unit)
        .and_then(|text| Reported::<Mass>::from_str(&text))
        .map_err(|source| NutritionError::Nutrient { id, source })?;
    Ok(measurement)
}

fn parse_energy(
    raw: &RawValue,
    loq: Option<&RawValue>,
    unit: &str,
    id: NutrientId,
) -> Result<Reported<Energy>, NutritionError> {
    let measurement = amount_with_unit(raw, loq, unit)
        .and_then(|text| Reported::<Energy>::from_str(&text))
        .map_err(|source| NutritionError::Nutrient { id, source })?;
    let expected_unit = if id == 1062 { "kJ" } else { "kcal" };
    let matches_expected = matches!(
        (measurement.amount(), expected_unit),
        (Energy::Kilocalories(_), "kcal") | (Energy::Kilojoules(_), "kJ")
    );
    if !matches_expected {
        return Err(NutritionError::Nutrient {
            id,
            source: MeasurementError::InvalidUnit,
        });
    }
    Ok(measurement)
}

fn amount_with_unit(
    raw: &RawValue,
    loq: Option<&RawValue>,
    unit: &str,
) -> Result<String, MeasurementError> {
    let amount = raw_number_text(raw)?;
    let unit = canonical_unit(unit);
    let is_numeric_zero = amount.parse::<Decimal>().is_ok_and(Decimal::is_zero);
    if is_numeric_zero && let Some(loq) = loq {
        let threshold = decimal_text(loq)?;
        if !threshold.parse::<Decimal>()?.is_zero() {
            return Ok(format!("< {threshold} {unit}"));
        }
    }
    Ok(format!("{amount} {unit}"))
}

fn decimal_text(raw: &RawValue) -> Result<String, MeasurementError> {
    Ok(raw_number_text(raw)?.parse::<Decimal>()?.to_string())
}

fn raw_number_text(raw: &RawValue) -> Result<String, MeasurementError> {
    let source = raw.get().trim();
    if source.starts_with('"') {
        let value =
            serde_json::from_str::<String>(source).map_err(|_| MeasurementError::InvalidDecimal)?;
        Ok(value.trim().to_owned())
    } else {
        Ok(source.to_owned())
    }
}

fn canonical_unit(unit: &str) -> String {
    match unit.to_ascii_lowercase().as_str() {
        "g" => "g".to_owned(),
        "mg" => "mg".to_owned(),
        "ug" | "µg" => "µg".to_owned(),
        "kcal" => "kcal".to_owned(),
        "kj" => "kJ".to_owned(),
        "ml" => "ml".to_owned(),
        _ => unit.to_owned(),
    }
}

fn branded_serving_unit(unit: &str) -> Option<&'static str> {
    match unit.to_ascii_lowercase().as_str() {
        "g" => Some("g"),
        "mg" => Some("mg"),
        "ug" | "µg" => Some("µg"),
        "ml" => Some("ml"),
        _ => None,
    }
}

fn nutrient_id(nutrient: &FoodNutrient) -> Option<NutrientId> {
    nutrient.nutrient.as_ref().and_then(|info| info.id)
}

fn preferred_nutrient<'food>(
    nutrients: &'food [FoodNutrient],
    ids: &[NutrientId],
) -> Result<Option<(NutrientId, &'food FoodNutrient, &'food RawValue)>, NutritionError> {
    for id in ids {
        let Some((nutrient, raw)) = nutrients.iter().find_map(|candidate| {
            if nutrient_id(candidate) != Some(*id) {
                return None;
            }
            candidate.amount.as_deref().map(|raw| (candidate, raw))
        }) else {
            continue;
        };
        let duplicate_count = nutrients
            .iter()
            .filter(|candidate| nutrient_id(candidate) == Some(*id) && candidate.amount.is_some())
            .count();
        if duplicate_count > 1 {
            return Err(NutritionError::DuplicateNutrient { id: *id });
        }
        return Ok(Some((*id, nutrient, raw)));
    }
    Ok(None)
}

fn serving_error(source: MeasurementError) -> NutritionError {
    NutritionError::Serving { source }
}

#[cfg(test)]
#[path = "../../../unit-tests/import/usda/nutrition.rs"]
mod tests;
