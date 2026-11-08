use std::borrow::Cow;
use std::collections::{BTreeMap, btree_map::Entry};

use crate::{DiagnosticField, DiagnosticReason};
use serde::Deserialize;
use serde_json::value::RawValue;

use crumbs_database::nutrition_model::{
    Energy, FoodData, Mass, MeasuredQuantity, Measurement, NutritionBasis, NutritionProfile,
    Qualifier, QuantityError, Reported, ServingSize,
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
        nutrient_id: NutrientId,
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

impl NutritionError {
    pub(crate) fn field(&self) -> Option<DiagnosticField> {
        let nutrient_id = match self {
            Self::MissingUnit { nutrient_id }
            | Self::IncompatibleUnit { nutrient_id, .. }
            | Self::DuplicateNutrient { nutrient_id }
            | Self::Amount { nutrient_id, .. } => Some(*nutrient_id),
            Self::Record(_)
            | Self::Scalar(_)
            | Self::UnsupportedBasis { .. }
            | Self::QualifiedLimitOfQuantification => None,
            Self::Serving { .. } => return Some("serving_sizes".to_owned()),
        };
        nutrient_id.map(|id| format!("nutrient[{id}]"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sanitization {
    pub(crate) field: DiagnosticField,
    pub(crate) reason: DiagnosticReason,
}

pub(crate) struct ParsedFoodData {
    pub(crate) food_data: FoodData,
    pub(crate) sanitizations: Vec<Sanitization>,
}

pub(crate) type NutrientId = u64;

pub(super) fn parse_food_data(raw: &str, kind: FoodKind) -> Result<ParsedFoodData, NutritionError> {
    let raw_record: RawNutritionRecord =
        serde_json::from_str(raw).map_err(NutritionError::Record)?;
    let mut sanitizations = Vec::new();
    let record = NutritionRecord {
        food_nutrients: parse_array(
            raw_record.food_nutrients.as_deref(),
            "foodNutrients",
            &mut sanitizations,
        ),
        food_portions: parse_array(
            raw_record.food_portions.as_deref(),
            "foodPortions",
            &mut sanitizations,
        ),
        serving_size: raw_record.serving_size,
        serving_size_unit: parse_optional(
            raw_record.serving_size_unit.as_deref(),
            "servingSizeUnit",
            &mut sanitizations,
        ),
        household_serving_full_text: parse_optional(
            raw_record.household_serving_full_text.as_deref(),
            "householdServingFullText",
            &mut sanitizations,
        ),
    };
    let serving_sizes = parse_servings(&record, kind, &mut sanitizations);
    let mut nutrients = record
        .food_nutrients
        .iter()
        .filter_map(|nutrient| {
            let metadata = nutrient.nutrient.as_ref()?;
            let classified = ClassifiedNutrient::from_id(metadata.id)?;
            (nutrient.amount.is_some() || nutrient.loq.is_some())
                .then_some((nutrient, metadata, classified))
        })
        .peekable();
    let basis = match nutrition_basis(&record, kind, nutrients.peek().is_some()) {
        Ok(basis) => basis,
        Err(error) => {
            omit(&mut sanitizations, "nutrition", error);
            None
        }
    };
    let Some(basis) = basis else {
        return Ok(ParsedFoodData {
            food_data: FoodData {
                nutrition: None,
                serving_sizes,
            },
            sanitizations,
        });
    };

    // A repeated source nutrient stays ambiguous even if a third value follows it.
    let mut amounts = BTreeMap::new();
    for (nutrient, metadata, classified) in nutrients {
        let amount = match parse_nutrient(nutrient, metadata, classified, &mut sanitizations) {
            Ok(Some(amount)) => amount,
            Ok(None) => continue,
            Err(error) => {
                omit(
                    &mut sanitizations,
                    &format!("nutrient[{}]", metadata.id),
                    error,
                );
                continue;
            }
        };
        match amounts.entry(classified) {
            Entry::Vacant(entry) => {
                entry.insert(Some(amount));
            }
            Entry::Occupied(mut entry) => {
                entry.insert(None);
                omit(
                    &mut sanitizations,
                    &format!("nutrient[{}]", metadata.id),
                    NutritionError::DuplicateNutrient {
                        nutrient_id: metadata.id,
                    },
                );
            }
        }
    }
    let mut profile = NutritionProfile::empty(basis);
    let mut has_amount = false;
    for amount in amounts.into_values().flatten() {
        has_amount = true;
        match amount {
            ParsedNutrient::Energy(amount) => {
                profile.energy.get_or_insert(amount);
            }
            ParsedNutrient::Sugar(amount) => {
                profile
                    .nutrients
                    .carbohydrates
                    .sugars
                    .total
                    .get_or_insert(amount);
            }
            ParsedNutrient::Mass(nutrient, amount) => {
                assign_nutrient(&mut profile, nutrient, amount)
            }
        }
    }
    Ok(ParsedFoodData {
        food_data: FoodData {
            nutrition: has_amount.then_some(profile),
            serving_sizes,
        },
        sanitizations,
    })
}

enum ParsedNutrient {
    Energy(Reported<Energy>),
    Sugar(Reported<Mass>),
    Mass(MassNutrient, Reported<Mass>),
}

fn parse_nutrient(
    nutrient: &FoodNutrient,
    metadata: &NutrientMetadata,
    classified: ClassifiedNutrient,
    sanitizations: &mut Vec<Sanitization>,
) -> Result<Option<ParsedNutrient>, NutritionError> {
    let unit = metadata.unit_name.as_deref().map(raw_scalar).transpose()?;
    let unit = unit.as_deref().ok_or(NutritionError::MissingUnit {
        nutrient_id: metadata.id,
    })?;
    match classified {
        ClassifiedNutrient::Energy(method) => {
            if normalize_energy_unit(unit) != Some(method.expected_unit()) {
                return Err(NutritionError::IncompatibleUnit {
                    nutrient_id: metadata.id,
                    unit: unit.to_owned(),
                });
            }
            Ok(parse_amount_and_limit::<Energy>(
                nutrient.amount.as_deref(),
                nutrient.loq.as_deref(),
                method.expected_unit(),
                metadata.id,
                "energy",
                sanitizations,
            )?
            .map(ParsedNutrient::Energy))
        }
        ClassifiedNutrient::TotalSugar(_) => Ok(parse_mass_amount(
            nutrient.amount.as_deref(),
            nutrient.loq.as_deref(),
            unit,
            metadata.id,
            "carbohydrates.sugars.total",
            sanitizations,
        )?
        .map(ParsedNutrient::Sugar)),
        ClassifiedNutrient::Mass(nutrient_type) => Ok(parse_mass_amount(
            nutrient.amount.as_deref(),
            nutrient.loq.as_deref(),
            unit,
            metadata.id,
            nutrient_type.field(),
            sanitizations,
        )?
        .map(|amount| ParsedNutrient::Mass(nutrient_type, amount))),
    }
}

fn omit(sanitizations: &mut Vec<Sanitization>, field: &str, error: impl std::fmt::Display) {
    sanitizations.push(Sanitization {
        field: field.to_owned(),
        reason: format!("{error}; omitted"),
    });
}

fn parse_optional<'a, T: Deserialize<'a>>(
    raw: Option<&'a RawValue>,
    field: &str,
    sanitizations: &mut Vec<Sanitization>,
) -> Option<T> {
    let raw = raw?;
    match serde_json::from_str(raw.get()) {
        Ok(value) => Some(value),
        Err(error) => {
            omit(sanitizations, field, error);
            None
        }
    }
}

fn parse_array<T: serde::de::DeserializeOwned>(
    raw: Option<&RawValue>,
    field: &str,
    sanitizations: &mut Vec<Sanitization>,
) -> Vec<T> {
    let entries: Vec<&RawValue> = parse_optional(raw, field, sanitizations).unwrap_or_default();
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match serde_json::from_str(entry.get()) {
            Ok(value) => Some(value),
            Err(error) => {
                omit(sanitizations, &format!("{field}[{index}]"), error);
                None
            }
        })
        .collect()
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
// Declaration order is the USDA energy preference order.
enum EnergyMethod {
    AtwaterSpecific,
    AtwaterGeneral,
    FoodEnergyKcal,
    FoodEnergyKj,
}

impl EnergyMethod {
    fn expected_unit(self) -> &'static str {
        match self {
            Self::AtwaterSpecific | Self::AtwaterGeneral | Self::FoodEnergyKcal => "kcal",
            Self::FoodEnergyKj => "kJ",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
// Declaration order prefers NLEA total sugars over other total sugars.
enum SugarMethod {
    TotalNlea,
    Total,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    fn field(self) -> &'static str {
        match self {
            Self::Protein => "protein",
            Self::TotalFat => "fat.total",
            Self::CarbohydrateByDifference => "carbohydrates.including_fibre",
            Self::Starch => "carbohydrates.starch",
            Self::Lactose => "carbohydrates.sugars.lactose",
            Self::Fibre => "fibre",
            Self::Sodium => "sodium",
            Self::AddedSugars => "carbohydrates.sugars.added",
            Self::Cholesterol => "cholesterol",
            Self::TransFat => "fat.trans",
            Self::SaturatedFat => "fat.saturated",
            Self::MonounsaturatedFat => "fat.monounsaturated",
            Self::PolyunsaturatedFat => "fat.polyunsaturated",
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
    sanitizations: &mut Vec<Sanitization>,
) -> Vec<ServingSize> {
    match kind {
        FoodKind::Branded => match branded_serving(record) {
            Ok(servings) => servings,
            Err(error) => {
                omit(sanitizations, "serving_sizes", error);
                Vec::new()
            }
        },
        FoodKind::Foundation | FoodKind::SrLegacy | FoodKind::Survey => {
            let mut servings = Vec::new();
            for (index, portion) in record.food_portions.iter().enumerate() {
                if let Some(weight) = portion.gram_weight.as_deref() {
                    match parse_food_portion(portion, weight) {
                        Ok(serving) => servings.push(serving),
                        Err(error) => {
                            omit(sanitizations, &format!("serving_sizes[{index}]"), error)
                        }
                    }
                }
            }
            servings
        }
    }
}

fn branded_serving(record: &NutritionRecord) -> Result<Vec<ServingSize>, NutritionError> {
    let Some(quantity) = record.serving_size.as_deref() else {
        return Ok(Vec::new());
    };
    let unit = record
        .serving_size_unit
        .as_deref()
        .ok_or_else(|| NutritionError::Serving {
            source: QuantityError::Unit("missing servingSizeUnit".to_owned()),
        })?;
    let canonical_unit = if unit.eq_ignore_ascii_case("g") {
        "g"
    } else if unit.eq_ignore_ascii_case("ml") {
        "ml"
    } else {
        return Err(NutritionError::Serving {
            source: QuantityError::Unit(unit.to_owned()),
        });
    };
    let quantity_text = format!("{} {canonical_unit}", raw_scalar(quantity)?);
    let quantity = quantity_text
        .parse::<MeasuredQuantity>()
        .map_err(|source| NutritionError::Serving { source })?;
    let name = record
        .household_serving_full_text
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| quantity.to_string());
    Ok(vec![
        ServingSize::new(name, quantity).map_err(|source| NutritionError::Serving { source })?,
    ])
}

fn parse_food_portion(
    portion: &FoodPortion,
    weight: &RawValue,
) -> Result<ServingSize, NutritionError> {
    let quantity = format!("{} g", raw_scalar(weight)?);
    let quantity = quantity
        .parse::<MeasuredQuantity>()
        .map_err(|source| NutritionError::Serving { source })?;
    let name = match portion
        .portion_description
        .as_deref()
        .filter(|description| !description.trim().is_empty())
    {
        Some(description) => description.to_owned(),
        None => portion_name(portion)?,
    };
    ServingSize::new(name, quantity).map_err(|source| NutritionError::Serving { source })
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
    field: &str,
    sanitizations: &mut Vec<Sanitization>,
) -> Result<Option<Reported<Mass>>, NutritionError> {
    let unit = normalize_mass_unit(unit).ok_or_else(|| NutritionError::IncompatibleUnit {
        nutrient_id,
        unit: unit.to_owned(),
    })?;
    parse_amount_and_limit(amount, loq, unit, nutrient_id, field, sanitizations)
}

fn normalize_energy_unit(unit: &str) -> Option<&'static str> {
    let unit = unit.trim();
    if unit.eq_ignore_ascii_case("kcal") {
        Some("kcal")
    } else if unit.eq_ignore_ascii_case("kJ") {
        Some("kJ")
    } else {
        None
    }
}

enum SourceAmount<T: Measurement> {
    Missing,
    Present(Reported<T>),
    Negative,
}

fn parse_amount_and_limit<T: Measurement>(
    amount: Option<&RawValue>,
    limit: Option<&RawValue>,
    unit: &str,
    nutrient_id: NutrientId,
    field: &str,
    sanitizations: &mut Vec<Sanitization>,
) -> Result<Option<Reported<T>>, NutritionError> {
    let source_amount = match amount.map(raw_scalar).transpose()? {
        Some(raw_amount) => match Reported::<T>::parse_with_unit(&raw_amount, unit) {
            Ok(amount) => SourceAmount::Present(amount),
            Err(QuantityError::Negative) => {
                sanitizations.push(Sanitization {
                    field: field.to_owned(),
                    reason: format!("negative source amount {raw_amount:?} was omitted"),
                });
                SourceAmount::Negative
            }
            Err(error) => {
                return Err(NutritionError::Amount {
                    nutrient_id,
                    value: format!("{raw_amount} {unit}"),
                    source: error,
                });
            }
        },
        None => SourceAmount::Missing,
    };

    let limit = match limit.map(raw_scalar).transpose()? {
        Some(raw_limit) => match parse_limit::<T>(&raw_limit, unit, nutrient_id)? {
            Some(limit) => Some(limit),
            None => {
                sanitizations.push(Sanitization {
                    field: field.to_owned(),
                    reason: format!(
                        "zero or negative limit of quantification {raw_limit:?} was omitted"
                    ),
                });
                None
            }
        },
        None => None,
    };

    Ok(match source_amount {
        SourceAmount::Negative => None,
        SourceAmount::Missing => limit,
        SourceAmount::Present(amount) => match limit {
            Some(limit)
                if amount.qualifier() == Qualifier::Exact && amount.amount().value().is_zero() =>
            {
                Some(limit)
            }
            _ => Some(amount),
        },
    })
}

fn parse_limit<T: Measurement>(
    value: &str,
    unit: &str,
    nutrient_id: NutrientId,
) -> Result<Option<Reported<T>>, NutritionError> {
    let parsed = match Reported::<T>::parse_with_unit(value, unit) {
        Ok(parsed) => parsed,
        Err(QuantityError::Negative) => return Ok(None),
        Err(source) => {
            return Err(NutritionError::Amount {
                nutrient_id,
                value: format!("{value} {unit}"),
                source,
            });
        }
    };
    if parsed.qualifier() != Qualifier::Exact {
        return Err(NutritionError::QualifiedLimitOfQuantification);
    }
    match Reported::new(Qualifier::LessThan, parsed.amount()) {
        Ok(limit) => Ok(Some(limit)),
        Err(QuantityError::ZeroStrictUpperBound) => Ok(None),
        Err(source) => Err(NutritionError::Amount {
            nutrient_id,
            value: format!("{value} {unit}"),
            source,
        }),
    }
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

fn assign_nutrient(profile: &mut NutritionProfile, nutrient: MassNutrient, amount: Reported<Mass>) {
    let slot = match nutrient {
        MassNutrient::Protein => &mut profile.nutrients.protein,
        MassNutrient::TotalFat => &mut profile.nutrients.fat.total,
        MassNutrient::CarbohydrateByDifference => {
            &mut profile.nutrients.carbohydrates.including_fibre
        }
        MassNutrient::Starch => &mut profile.nutrients.carbohydrates.starch,
        MassNutrient::Lactose => &mut profile.nutrients.carbohydrates.sugars.lactose,
        MassNutrient::Fibre => &mut profile.nutrients.fibre,
        MassNutrient::Sodium => &mut profile.nutrients.sodium,
        MassNutrient::AddedSugars => &mut profile.nutrients.carbohydrates.sugars.added,
        MassNutrient::Cholesterol => &mut profile.nutrients.cholesterol,
        MassNutrient::TransFat => &mut profile.nutrients.fat.trans,
        MassNutrient::SaturatedFat => &mut profile.nutrients.fat.saturated,
        MassNutrient::MonounsaturatedFat => &mut profile.nutrients.fat.monounsaturated,
        MassNutrient::PolyunsaturatedFat => &mut profile.nutrients.fat.polyunsaturated,
    };
    *slot = Some(amount);
}

struct NutritionRecord {
    food_nutrients: Vec<FoodNutrient>,
    food_portions: Vec<FoodPortion>,
    serving_size: Option<Box<RawValue>>,
    serving_size_unit: Option<String>,
    household_serving_full_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawNutritionRecord {
    #[serde(default, rename = "foodNutrients")]
    food_nutrients: Option<Box<RawValue>>,
    #[serde(default, rename = "foodPortions")]
    food_portions: Option<Box<RawValue>>,
    #[serde(default, rename = "servingSize")]
    serving_size: Option<Box<RawValue>>,
    #[serde(default, rename = "servingSizeUnit")]
    serving_size_unit: Option<Box<RawValue>>,
    #[serde(default, rename = "householdServingFullText")]
    household_serving_full_text: Option<Box<RawValue>>,
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
#[path = "../unit-tests/usda_nutrition.rs"]
mod tests;
