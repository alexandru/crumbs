use std::borrow::Cow;
use std::str::FromStr;

use serde::Deserialize;
use serde_json::value::RawValue;

use crate::nutrition::{
    Energy, FoodData, Mass, NutritionBasis, NutritionProfile, Qualifier, QuantityError, Reported,
    ServingQuantity, ServingSize,
};

use super::FoodKind;

#[derive(Debug, thiserror::Error)]
pub(crate) enum NutritionError {
    #[error("malformed nutrition source data: {0}")]
    Record(#[source] serde_json::Error),
    #[error("invalid scalar value: {0}")]
    Scalar(#[source] serde_json::Error),
    #[error("nutrient {nutrient_id} has no unit")]
    MissingUnit { nutrient_id: NutrientId },
    #[error("nutrient {nutrient_id} has incompatible unit {unit:?}")]
    IncompatibleUnit {
        nutrient_id: NutrientId,
        unit: String,
    },
    #[error("nutrient amount {value:?} is invalid: {source}")]
    Amount {
        value: String,
        #[source]
        source: QuantityError,
    },
    #[error("multiple source values map to nutrient {nutrient_id}")]
    DuplicateNutrient { nutrient_id: NutrientId },
    #[error("branded nutrition has unsupported 100-unit basis {unit:?}")]
    UnsupportedBasis { unit: Option<String> },
    #[error("limit of quantification must be an exact measured amount")]
    QualifiedLimitOfQuantification,
    #[error("invalid serving quantity: {source}")]
    Serving {
        #[source]
        source: QuantityError,
    },
}

pub(super) fn parse_food_data(raw: &str, kind: FoodKind) -> Result<FoodData, NutritionError> {
    let record: NutritionRecord = serde_json::from_str(raw).map_err(NutritionError::Record)?;
    let serving_sizes = parse_servings(&record, kind)?;
    let food_nutrients = record.food_nutrients.as_deref().unwrap_or(&[]);
    let recognized = food_nutrients.iter().any(|nutrient| {
        nutrient.nutrient.as_ref().is_some_and(|metadata| {
            ClassifiedNutrient::from_id(metadata.id).is_some()
                && (nutrient.amount.is_some() || nutrient.loq.is_some())
        })
    });

    let Some(basis) = nutrition_basis(&record, kind, recognized)? else {
        return Ok(FoodData {
            nutrition: None,
            serving_sizes,
        });
    };

    let mut profile = NutritionProfile::empty(basis);
    let mut has_amount = false;
    let mut energy_candidates: Vec<(EnergyMethod, Reported<Energy>)> = Vec::new();
    let mut total_sugar_candidates: Vec<(SugarMethod, Reported<Mass>)> = Vec::new();
    for nutrient in food_nutrients {
        let Some(metadata) = nutrient.nutrient.as_ref() else {
            continue;
        };
        let Some(classified) = ClassifiedNutrient::from_id(metadata.id) else {
            continue;
        };
        if nutrient.amount.is_none() && nutrient.loq.is_none() {
            continue;
        }
        let unit = metadata.unit_name.as_deref().map(raw_scalar).transpose()?;
        let unit = unit.as_deref().ok_or(NutritionError::MissingUnit {
            nutrient_id: metadata.id,
        })?;
        match classified {
            ClassifiedNutrient::Energy(method) => {
                let Some(energy) = parse_energy_amount(
                    nutrient.amount.as_deref(),
                    nutrient.loq.as_deref(),
                    unit,
                    metadata.id,
                )?
                else {
                    continue;
                };
                if energy_unit(energy.amount) != method.expected_unit() {
                    return Err(NutritionError::IncompatibleUnit {
                        nutrient_id: metadata.id,
                        unit: unit.to_owned(),
                    });
                }
                energy_candidates.push((method, energy));
            }
            ClassifiedNutrient::TotalSugar(method) => {
                let Some(amount) = parse_mass_amount(
                    nutrient.amount.as_deref(),
                    nutrient.loq.as_deref(),
                    unit,
                    metadata.id,
                )?
                else {
                    continue;
                };
                has_amount = true;
                if total_sugar_candidates
                    .iter()
                    .any(|(candidate, _)| *candidate == method)
                {
                    return Err(NutritionError::DuplicateNutrient {
                        nutrient_id: metadata.id,
                    });
                }
                total_sugar_candidates.push((method, amount));
            }
            ClassifiedNutrient::Mass(nutrient_type) => {
                let Some(amount) = parse_mass_amount(
                    nutrient.amount.as_deref(),
                    nutrient.loq.as_deref(),
                    unit,
                    metadata.id,
                )?
                else {
                    continue;
                };
                has_amount = true;
                assign_nutrient(&mut profile, nutrient_type, amount)?;
            }
        }
    }

    // Prefer source ID 1063 over 2000 and never sum the two source values.
    profile.carbohydrates.sugars.total = total_sugar_candidates
        .iter()
        .find(|(method, _)| *method == SugarMethod::TotalNlea)
        .or_else(|| {
            total_sugar_candidates
                .iter()
                .find(|(method, _)| *method == SugarMethod::Total)
        })
        .map(|(_, amount)| amount.clone());
    profile.energy = select_energy(energy_candidates)?;
    let nutrition = (has_amount || profile.energy.is_some()).then_some(profile);
    Ok(FoodData {
        nutrition,
        serving_sizes,
    })
}

pub(crate) type NutrientId = u64;
type EnergyPriority = u8;

const PROTEIN: NutrientId = 1003;
const TOTAL_FAT: NutrientId = 1004;
const CARBOHYDRATE_BY_DIFFERENCE: NutrientId = 1005;
const ENERGY_KILOCALORIES: NutrientId = 1008;
const STARCH: NutrientId = 1009;
const LACTOSE: NutrientId = 1013;
const ENERGY_KILOJOULES: NutrientId = 1062;
const SUGARS_TOTAL_NLEA: NutrientId = 1063;
const FIBRE: NutrientId = 1079;
const SODIUM: NutrientId = 1093;
const ADDED_SUGARS: NutrientId = 1235;
const CHOLESTEROL: NutrientId = 1253;
const TRANS_FAT: NutrientId = 1257;
const SATURATED_FAT: NutrientId = 1258;
const MONOUNSATURATED_FAT: NutrientId = 1292;
const POLYUNSATURATED_FAT: NutrientId = 1293;
const SUGARS_TOTAL: NutrientId = 2000;
const ATWATER_GENERAL: NutrientId = 2047;
const ATWATER_SPECIFIC: NutrientId = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClassifiedNutrient {
    Energy(EnergyMethod),
    TotalSugar(SugarMethod),
    Mass(MassNutrient),
}

impl ClassifiedNutrient {
    fn from_id(id: NutrientId) -> Option<Self> {
        match id {
            ATWATER_SPECIFIC => Some(Self::Energy(EnergyMethod::AtwaterSpecific)),
            ATWATER_GENERAL => Some(Self::Energy(EnergyMethod::AtwaterGeneral)),
            ENERGY_KILOCALORIES => Some(Self::Energy(EnergyMethod::FoodEnergyKcal)),
            ENERGY_KILOJOULES => Some(Self::Energy(EnergyMethod::FoodEnergyKj)),
            SUGARS_TOTAL_NLEA => Some(Self::TotalSugar(SugarMethod::TotalNlea)),
            SUGARS_TOTAL => Some(Self::TotalSugar(SugarMethod::Total)),
            PROTEIN => Some(Self::Mass(MassNutrient::Protein)),
            TOTAL_FAT => Some(Self::Mass(MassNutrient::TotalFat)),
            CARBOHYDRATE_BY_DIFFERENCE => Some(Self::Mass(MassNutrient::CarbohydrateByDifference)),
            STARCH => Some(Self::Mass(MassNutrient::Starch)),
            LACTOSE => Some(Self::Mass(MassNutrient::Lactose)),
            FIBRE => Some(Self::Mass(MassNutrient::Fibre)),
            SODIUM => Some(Self::Mass(MassNutrient::Sodium)),
            ADDED_SUGARS => Some(Self::Mass(MassNutrient::AddedSugars)),
            CHOLESTEROL => Some(Self::Mass(MassNutrient::Cholesterol)),
            TRANS_FAT => Some(Self::Mass(MassNutrient::TransFat)),
            SATURATED_FAT => Some(Self::Mass(MassNutrient::SaturatedFat)),
            MONOUNSATURATED_FAT => Some(Self::Mass(MassNutrient::MonounsaturatedFat)),
            POLYUNSATURATED_FAT => Some(Self::Mass(MassNutrient::PolyunsaturatedFat)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnergyMethod {
    AtwaterSpecific,
    AtwaterGeneral,
    FoodEnergyKcal,
    FoodEnergyKj,
}

impl EnergyMethod {
    fn id(self) -> NutrientId {
        match self {
            Self::AtwaterSpecific => ATWATER_SPECIFIC,
            Self::AtwaterGeneral => ATWATER_GENERAL,
            Self::FoodEnergyKcal => ENERGY_KILOCALORIES,
            Self::FoodEnergyKj => ENERGY_KILOJOULES,
        }
    }

    fn expected_unit(self) -> &'static str {
        match self {
            Self::AtwaterSpecific | Self::AtwaterGeneral | Self::FoodEnergyKcal => "kcal",
            Self::FoodEnergyKj => "kJ",
        }
    }

    fn priority(self) -> EnergyPriority {
        match self {
            Self::AtwaterSpecific => 0,
            Self::AtwaterGeneral => 1,
            Self::FoodEnergyKcal => 2,
            Self::FoodEnergyKj => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SugarMethod {
    TotalNlea,
    Total,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MassNutrient {
    Protein,
    TotalFat,
    CarbohydrateByDifference,
    Starch,
    Lactose,
    Fibre,
    Sodium,
    AddedSugars,
    Cholesterol,
    TransFat,
    SaturatedFat,
    MonounsaturatedFat,
    PolyunsaturatedFat,
}

impl MassNutrient {
    fn id(self) -> NutrientId {
        match self {
            Self::Protein => PROTEIN,
            Self::TotalFat => TOTAL_FAT,
            Self::CarbohydrateByDifference => CARBOHYDRATE_BY_DIFFERENCE,
            Self::Starch => STARCH,
            Self::Lactose => LACTOSE,
            Self::Fibre => FIBRE,
            Self::Sodium => SODIUM,
            Self::AddedSugars => ADDED_SUGARS,
            Self::Cholesterol => CHOLESTEROL,
            Self::TransFat => TRANS_FAT,
            Self::SaturatedFat => SATURATED_FAT,
            Self::MonounsaturatedFat => MONOUNSATURATED_FAT,
            Self::PolyunsaturatedFat => POLYUNSATURATED_FAT,
        }
    }
}

fn nutrition_basis(
    record: &NutritionRecord,
    kind: FoodKind,
    recognized_nutrient: bool,
) -> Result<Option<NutritionBasis>, NutritionError> {
    if !recognized_nutrient {
        return Ok(None);
    }
    match kind {
        FoodKind::Foundation | FoodKind::SrLegacy | FoodKind::Survey => {
            Ok(Some(NutritionBasis::Per100Grams))
        }
        FoodKind::Branded => match record.serving_size_unit.as_deref() {
            Some(unit) if unit.eq_ignore_ascii_case("g") => Ok(Some(NutritionBasis::Per100Grams)),
            Some(unit) if unit.eq_ignore_ascii_case("ml") => {
                Ok(Some(NutritionBasis::Per100Millilitres))
            }
            unit => Err(NutritionError::UnsupportedBasis {
                unit: unit.map(str::to_owned),
            }),
        },
    }
}

fn parse_servings(
    record: &NutritionRecord,
    kind: FoodKind,
) -> Result<Vec<ServingSize>, NutritionError> {
    match kind {
        FoodKind::Branded => branded_serving(record),
        FoodKind::Foundation | FoodKind::SrLegacy | FoodKind::Survey => {
            let mut servings = Vec::new();
            for portion in record.food_portions.iter().flatten() {
                if let Some(weight) = portion.gram_weight.as_deref() {
                    servings.push(parse_food_portion(portion, weight)?);
                }
            }
            Ok(servings)
        }
    }
}

fn branded_serving(record: &NutritionRecord) -> Result<Vec<ServingSize>, NutritionError> {
    let (Some(quantity), Some(unit)) = (
        record.serving_size.as_deref(),
        record.serving_size_unit.as_deref(),
    ) else {
        return Ok(Vec::new());
    };
    let canonical_unit = if unit.eq_ignore_ascii_case("g") {
        "g"
    } else if unit.eq_ignore_ascii_case("ml") {
        "ml"
    } else {
        return Ok(Vec::new());
    };
    let quantity_text = format!("{} {canonical_unit}", raw_scalar(quantity)?);
    let quantity = quantity_text
        .parse::<ServingQuantity>()
        .map_err(|source| NutritionError::Serving { source })?;
    let name = record
        .household_serving_full_text
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| quantity.to_string());
    Ok(vec![ServingSize { name, quantity }])
}

fn parse_food_portion(
    portion: &FoodPortion,
    weight: &RawValue,
) -> Result<ServingSize, NutritionError> {
    let quantity = format!("{} g", raw_scalar(weight)?);
    let quantity = quantity
        .parse::<ServingQuantity>()
        .map_err(|source| NutritionError::Serving { source })?;
    let name = match portion
        .portion_description
        .as_deref()
        .filter(|description| !description.trim().is_empty())
    {
        Some(description) => description.to_owned(),
        None => portion_name(portion)?,
    };
    Ok(ServingSize { name, quantity })
}

fn portion_name(portion: &FoodPortion) -> Result<String, NutritionError> {
    let amount = portion.amount.as_deref().map(raw_scalar).transpose()?;
    let measure = portion
        .measure_unit
        .as_ref()
        .and_then(|unit| unit.name.as_deref())
        .filter(|name| !name.trim().is_empty() && !name.eq_ignore_ascii_case("undetermined"));
    let modifier = portion
        .modifier
        .as_deref()
        .filter(|modifier| !modifier.trim().is_empty());
    match (amount, measure, modifier) {
        (Some(amount), Some(measure), Some(modifier)) => {
            Ok(format!("{amount} {modifier} {measure}"))
        }
        (Some(amount), Some(measure), None) => Ok(format!("{amount} {measure}")),
        (Some(amount), None, Some(modifier)) => Ok(format!("{amount} {modifier}")),
        (Some(amount), None, None) => Ok(format!("{amount} portion")),
        (None, Some(measure), Some(modifier)) => Ok(format!("{modifier} {measure}")),
        (None, Some(measure), None) => Ok(measure.to_owned()),
        (None, None, Some(modifier)) => Ok(modifier.to_owned()),
        (None, None, None) => Ok("portion".to_owned()),
    }
}

fn parse_mass_amount(
    amount: Option<&RawValue>,
    loq: Option<&RawValue>,
    unit: &str,
    nutrient_id: NutrientId,
) -> Result<Option<Reported<Mass>>, NutritionError> {
    let unit = normalize_mass_unit(unit).ok_or_else(|| NutritionError::IncompatibleUnit {
        nutrient_id,
        unit: unit.to_owned(),
    })?;
    parse_amount_and_limit(amount, loq, unit, mass_is_zero)
}

fn parse_energy_amount(
    amount: Option<&RawValue>,
    loq: Option<&RawValue>,
    unit: &str,
    nutrient_id: NutrientId,
) -> Result<Option<Reported<Energy>>, NutritionError> {
    let normalized = match unit.trim().to_ascii_lowercase().as_str() {
        "kcal" => "kcal",
        "kj" => "kJ",
        _ => {
            return Err(NutritionError::IncompatibleUnit {
                nutrient_id,
                unit: unit.to_owned(),
            });
        }
    };
    parse_amount_and_limit(amount, loq, normalized, energy_is_zero)
}

fn parse_amount_and_limit<T>(
    amount: Option<&RawValue>,
    limit: Option<&RawValue>,
    unit: &str,
    is_zero: impl Fn(&T) -> bool,
) -> Result<Option<Reported<T>>, NutritionError>
where
    T: FromStr<Err = QuantityError>,
{
    let parsed = amount
        .map(raw_scalar)
        .transpose()?
        .map(|source| reported_with_unit::<T>(&source, unit))
        .transpose()?;
    let limit = limit
        .map(raw_scalar)
        .transpose()?
        .map(|source| reported_with_unit::<T>(&source, unit))
        .transpose()?;
    let limit = limit
        .map(|limit| {
            if limit.qualifier == Qualifier::Exact {
                Ok(limit.amount)
            } else {
                Err(NutritionError::QualifiedLimitOfQuantification)
            }
        })
        .transpose()?;
    Ok(with_limit_of_quantification(parsed, limit, is_zero))
}

fn with_limit_of_quantification<T>(
    amount: Option<Reported<T>>,
    limit: Option<T>,
    is_zero: impl Fn(&T) -> bool,
) -> Option<Reported<T>> {
    let Some(limit) = limit else {
        return amount;
    };
    let below_limit = amount
        .as_ref()
        .is_none_or(|amount| amount.qualifier == Qualifier::Exact && is_zero(&amount.amount));
    if below_limit {
        Some(Reported {
            qualifier: Qualifier::LessThan,
            amount: limit,
        })
    } else {
        amount
    }
}

fn mass_is_zero(amount: &Mass) -> bool {
    amount.is_zero()
}

fn energy_is_zero(amount: &Energy) -> bool {
    amount.is_zero()
}

fn reported_with_unit<T: FromStr<Err = QuantityError>>(
    value: &str,
    unit: &str,
) -> Result<Reported<T>, NutritionError> {
    let value = value.trim();
    let value = format!("{value} {unit}");
    value
        .parse()
        .map_err(|source| NutritionError::Amount { value, source })
}

fn raw_scalar(raw: &RawValue) -> Result<Cow<'_, str>, NutritionError> {
    let value = raw.get().trim();
    if value.starts_with('"') {
        serde_json::from_str(value)
            .map(Cow::Owned)
            .map_err(NutritionError::Scalar)
    } else {
        Ok(Cow::Borrowed(value))
    }
}

fn normalize_mass_unit(unit: &str) -> Option<&'static str> {
    match unit.trim().to_ascii_lowercase().as_str() {
        "g" | "gm" => Some("g"),
        "mg" => Some("mg"),
        "µg" | "μg" | "ug" | "mcg" => Some("µg"),
        _ => None,
    }
}

fn energy_unit(energy: Energy) -> &'static str {
    match energy {
        Energy::Kilocalories(_) => "kcal",
        Energy::Kilojoules(_) => "kJ",
    }
}

fn select_energy(
    candidates: Vec<(EnergyMethod, Reported<Energy>)>,
) -> Result<Option<Reported<Energy>>, NutritionError> {
    let mut selected: Option<(EnergyPriority, Reported<Energy>)> = None;
    let mut seen = Vec::new();
    for (method, value) in candidates {
        if seen.contains(&method) {
            return Err(NutritionError::DuplicateNutrient {
                nutrient_id: method.id(),
            });
        }
        seen.push(method);
        let priority = method.priority();
        if selected
            .as_ref()
            .is_none_or(|(selected_priority, _)| priority < *selected_priority)
        {
            selected = Some((priority, value));
        }
    }
    Ok(selected.map(|(_, value)| value))
}

fn assign_nutrient(
    profile: &mut NutritionProfile,
    nutrient: MassNutrient,
    amount: Reported<Mass>,
) -> Result<(), NutritionError> {
    let slot = match nutrient {
        MassNutrient::Protein => &mut profile.protein,
        MassNutrient::TotalFat => &mut profile.fat.total,
        MassNutrient::CarbohydrateByDifference => &mut profile.carbohydrates.including_fibre,
        MassNutrient::Starch => &mut profile.carbohydrates.starch,
        MassNutrient::Lactose => &mut profile.carbohydrates.sugars.lactose,
        MassNutrient::Fibre => &mut profile.fibre,
        MassNutrient::Sodium => &mut profile.sodium,
        MassNutrient::AddedSugars => &mut profile.carbohydrates.sugars.added,
        MassNutrient::Cholesterol => &mut profile.cholesterol,
        MassNutrient::TransFat => &mut profile.fat.trans,
        MassNutrient::SaturatedFat => &mut profile.fat.saturated,
        MassNutrient::MonounsaturatedFat => &mut profile.fat.monounsaturated,
        MassNutrient::PolyunsaturatedFat => &mut profile.fat.polyunsaturated,
    };
    if slot.is_some() {
        return Err(NutritionError::DuplicateNutrient {
            nutrient_id: nutrient.id(),
        });
    }
    *slot = Some(amount);
    Ok(())
}

#[derive(Debug, Deserialize)]
struct NutritionRecord {
    #[serde(default, rename = "foodNutrients")]
    food_nutrients: Option<Vec<FoodNutrient>>,
    #[serde(default, rename = "foodPortions")]
    food_portions: Option<Vec<FoodPortion>>,
    #[serde(default, rename = "servingSize")]
    serving_size: Option<Box<RawValue>>,
    #[serde(default, rename = "servingSizeUnit")]
    serving_size_unit: Option<String>,
    #[serde(default, rename = "householdServingFullText")]
    household_serving_full_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FoodNutrient {
    #[serde(default)]
    nutrient: Option<NutrientMetadata>,
    #[serde(default)]
    amount: Option<Box<RawValue>>,
    #[serde(default)]
    loq: Option<Box<RawValue>>,
}

#[derive(Debug, Deserialize)]
struct NutrientMetadata {
    #[serde(default)]
    id: NutrientId,
    #[serde(default, rename = "unitName")]
    unit_name: Option<Box<RawValue>>,
}

#[derive(Debug, Deserialize)]
struct FoodPortion {
    #[serde(default, rename = "portionDescription")]
    portion_description: Option<String>,
    #[serde(default, rename = "gramWeight")]
    gram_weight: Option<Box<RawValue>>,
    #[serde(default)]
    amount: Option<Box<RawValue>>,
    #[serde(default)]
    modifier: Option<String>,
    #[serde(default, rename = "measureUnit")]
    measure_unit: Option<MeasureUnit>,
}

#[derive(Debug, Deserialize)]
struct MeasureUnit {
    #[serde(default)]
    name: Option<String>,
}

#[cfg(test)]
#[path = "../../unit-tests/import/usda_nutrition.rs"]
mod tests;
