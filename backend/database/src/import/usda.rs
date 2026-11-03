//! Streaming parser for the official USDA FoodData Central JSON archives.
//!
//! The archives wrap a single array under one root key. This module validates
//! the root and each food record while forwarding it as a [`PreparedFood`]
//! without collecting the array or a `serde_json::Value`. Alongside the header
//! fields, every record is parsed into the shared [`ParsedFoodData`] model, so
//! nutrition and serving data never leave this module as provider JSON.
//!
//! All amounts are parsed as exact fixed-point decimals from the raw JSON
//! number text; no value passes through binary floating point.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;

use serde::Deserialize;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Deserializer;
use serde_json::Number as JsonNumber;
use serde_json::value::RawValue;
use tokio::sync::mpsc;

use crate::Source;
use crate::foods::Food;
use crate::measure::{
    Decimal, Energy, Grams, Kilocalories, Kilojoules, Mass, MassUnit, MeasureError, MeasuredMass,
    MeasuredQuantity, Milligrams, decimal_from_json_number, measured_decimal_from_json_number,
    unit_matches,
};
use crate::parsed::{
    Carbohydrates, Fat, NutritionBasis, NutritionProfile, ParsedFoodData, ServingName, ServingSize,
    Sugars, reports_any_amount,
};

/// A validated food record ready for persistence.
#[derive(Debug)]
pub(crate) struct PreparedFood {
    /// Positive FoodData Central identifier, converted to decimal text.
    pub(crate) fdc_id: FdcId,
    /// Food kind declared by `dataType` and checked against the root key.
    pub(crate) kind: FoodKind,
    /// Nonblank food description, preserved verbatim.
    pub(crate) description: FoodDescription,
    /// Optional category text from `foodCategory.description`,
    /// `wweiaFoodCategory.wweiaFoodCategoryDescription`, or `brandedFoodCategory`.
    pub(crate) category: Option<FoodCategory>,
    /// Source-independent nutrition and serving data parsed from the record.
    pub(crate) parsed: ParsedFoodData,
    /// One-based position of the record within the root array.
    pub(crate) position: RecordPosition,
}

impl PreparedFood {
    pub(crate) fn into_food(self) -> Food {
        Food {
            source: USDA_SOURCE,
            source_food_id: self.fdc_id.0,
            food_type: self.kind.as_str().into(),
            description: self.description.0,
            category: self.category.map(|category| category.0),
            parsed_data: self.parsed,
        }
    }
}

/// A positive FoodData Central identifier represented as decimal text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FdcId(String);

impl FdcId {
    fn from_json(value: RawFdcId) -> Result<Self, RecordError> {
        let value = NonZeroU64::new(value).ok_or(RecordError::ZeroFdcId)?;
        Ok(Self(value.to_string()))
    }
}

impl fmt::Display for FdcId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Food kind declared by the `dataType` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum FoodKind {
    /// Foundation Foods archive.
    #[serde(rename = "Foundation")]
    Foundation,
    /// SR Legacy archive.
    #[serde(rename = "SR Legacy")]
    SrLegacy,
    /// Survey (FNDDS) archive.
    #[serde(rename = "Survey (FNDDS)")]
    Survey,
    /// Branded Foods archive.
    #[serde(rename = "Branded")]
    Branded,
}

impl FoodKind {
    /// Returns the exact `dataType` spelling for this kind.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Foundation => "Foundation",
            Self::SrLegacy => "SR Legacy",
            Self::Survey => "Survey (FNDDS)",
            Self::Branded => "Branded",
        }
    }
}

impl fmt::Display for FoodKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A nonblank food description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoodDescription(String);

impl FoodDescription {
    fn from_json(value: RawFoodDescription) -> Result<Self, RecordError> {
        if value.trim().is_empty() {
            return Err(RecordError::BlankDescription);
        }
        Ok(Self(value))
    }
}

impl fmt::Display for FoodDescription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Optional category text attached to a food.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoodCategory(String);

impl fmt::Display for FoodCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One-based position of a record within the root food array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RecordPosition(NonZeroUsize);

impl RecordPosition {
    const fn first() -> Self {
        Self(NonZeroUsize::MIN)
    }

    fn checked_next(self) -> Option<Self> {
        self.0
            .get()
            .checked_add(1)
            .and_then(NonZeroUsize::new)
            .map(Self)
    }
}

impl fmt::Display for RecordPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Errors produced while streaming USDA FoodData Central JSON.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ParseError {
    /// The document framing is invalid: wrong root, missing array, or trailing
    /// content. `source` keeps serde's line and column.
    #[error("invalid USDA FoodData Central document: {source}")]
    Document {
        /// Underlying JSON error with line and column information.
        #[source]
        source: serde_json::Error,
    },
    /// Reading document framing or trailing input failed because of an input I/O
    /// error.
    #[error("failed to read USDA FoodData Central document: {source}")]
    Read {
        /// Underlying I/O error while reading document framing or trailing input.
        #[source]
        source: serde_json::Error,
    },
    /// A food record cannot be read or is invalid. `source` identifies the read
    /// or field problem.
    #[error("invalid USDA FoodData Central record {position}: {source}")]
    Record {
        /// One-based record position.
        position: RecordPosition,
        /// Record-level read or validation failure.
        #[source]
        source: RecordError,
    },
    /// The receiving side of the bounded channel disappeared.
    #[error("consumer disconnected at USDA FoodData Central record {position}")]
    ConsumerDisconnected {
        /// Record that could not be delivered.
        position: RecordPosition,
    },
    /// The record counter could not represent another record.
    #[error("record count overflowed at USDA FoodData Central record {position}")]
    RecordCountOverflow {
        /// Last representable record position.
        position: RecordPosition,
    },
}

impl ParseError {
    fn document(source: serde_json::Error) -> Self {
        if source.is_io() {
            Self::Read { source }
        } else {
            Self::Document { source }
        }
    }
}

/// Details of reading or validating a food record.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RecordError {
    /// The record is not a valid header or nutrition object. Serde's line and
    /// column are relative to the start of the record object.
    #[error("malformed food record: {0}")]
    Header(#[from] serde_json::Error),
    /// The record JSON is malformed. Serde's line and column are relative to
    /// the archive document.
    #[error("malformed record JSON: {0}")]
    Syntax(#[source] serde_json::Error),
    /// Reading the record JSON failed because of an input I/O error.
    #[error("failed to read record JSON: {0}")]
    Read(#[source] serde_json::Error),
    /// The record was valid JSON but not an object.
    #[error("food record must be a JSON object")]
    NotObject,
    /// `fdcId` was zero.
    #[error("fdcId must be positive")]
    ZeroFdcId,
    /// `description` was whitespace-only.
    #[error("description must not be blank")]
    BlankDescription,
    /// The root key and `dataType` disagree.
    #[error("dataType {data_type} does not match the {root} root")]
    KindMismatch {
        /// Kind implied by the root key.
        root: FoodKind,
        /// Kind declared by the record.
        data_type: FoodKind,
    },
    /// A covered nutrient's amount could not be parsed as an exact decimal.
    #[error("invalid amount for nutrient {nutrient_id}: {source}")]
    NutrientAmount {
        /// FoodData Central nutrient identifier of the rejected amount.
        nutrient_id: NutrientId,
        /// The exact-decimal parse failure.
        #[source]
        source: MeasureError,
    },
    /// A covered nutrient is reported in a unit that is not supported for its
    /// dimension.
    #[error("unsupported unit `{unit}` for nutrient {nutrient_id}")]
    NutrientUnit {
        /// FoodData Central nutrient identifier of the rejected entry.
        nutrient_id: NutrientId,
        /// The rejected unit name.
        unit: String,
    },
    /// A covered nutrient is reported twice in one record.
    #[error("duplicate nutrient {nutrient_id} in one record")]
    DuplicateNutrient {
        /// FoodData Central nutrient identifier reported twice.
        nutrient_id: NutrientId,
    },
    /// A branded label nutrient value could not be parsed as an exact decimal.
    #[error("invalid amount for branded label nutrient {label}: {source}")]
    LabelNutrient {
        /// Name of the rejected `labelNutrients` key.
        label: &'static str,
        /// The exact-decimal parse failure.
        #[source]
        source: MeasureError,
    },
}

/// Streams food records from `reader` into `sender`.
///
/// The input must be a single top-level object with exactly one recognized
/// array: `FoundationFoods`, `SRLegacyFoods`, `SurveyFoods`, or `BrandedFoods`.
/// The parser validates each record, parses its nutrition and serving data,
/// and never collects the array.
///
/// The function uses [`mpsc::Sender::blocking_send`], so it must run on a
/// blocking thread, such as one created by [`tokio::task::spawn_blocking`],
/// rather than on an async executor thread.
///
/// # Errors
///
/// Returns [`ParseError::Document`] for invalid root framing or trailing
/// content, [`ParseError::Read`] for document-level input I/O failures,
/// [`ParseError::Record`] for unreadable or invalid records, and
/// [`ParseError::ConsumerDisconnected`] when the receiver is dropped.
pub(crate) fn stream_foods(
    reader: impl Read,
    sender: &mpsc::Sender<PreparedFood>,
) -> Result<(), ParseError> {
    let mut deserializer = Deserializer::from_reader(reader);
    let mut state = StreamState::Ready;
    let result = RootSeed {
        sender,
        state: &mut state,
    }
    .deserialize(&mut deserializer);

    match (result, state) {
        (_, StreamState::Failure(failure)) => Err(failure),
        (Ok(()), StreamState::Ready | StreamState::Record(_)) => {
            deserializer.end().map_err(ParseError::document)
        }
        (Err(source), StreamState::Record(position)) => {
            let source = if source.is_io() {
                RecordError::Read(source)
            } else {
                RecordError::Syntax(source)
            };
            Err(ParseError::Record { position, source })
        }
        (Err(source), StreamState::Ready) => Err(ParseError::document(source)),
    }
}

const FOUNDATION_ROOT: &str = "FoundationFoods";
const SR_LEGACY_ROOT: &str = "SRLegacyFoods";
const SURVEY_ROOT: &str = "SurveyFoods";
const BRANDED_ROOT: &str = "BrandedFoods";
const ROOT_NAMES: &str = "FoundationFoods, SRLegacyFoods, SurveyFoods, or BrandedFoods";
const PROCESSING_FAILED: &str = "usda fooddata central processing failed";
/// USDA's placeholder measure unit for portions without a real unit.
const UNDETERMINED_UNIT: &str = "undetermined";

enum StreamState {
    Ready,
    Failure(ParseError),
    Record(RecordPosition),
}

struct RootSeed<'a> {
    sender: &'a mpsc::Sender<PreparedFood>,
    state: &'a mut StreamState,
}

impl<'de> DeserializeSeed<'de> for RootSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(RootVisitor {
            sender: self.sender,
            state: self.state,
        })
    }
}

struct RootVisitor<'a> {
    sender: &'a mpsc::Sender<PreparedFood>,
    state: &'a mut StreamState,
}

impl<'de> Visitor<'de> for RootVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "one USDA FoodData Central root object containing one food array"
        )
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let RootVisitor { sender, state } = self;
        let Some(root) = map.next_key::<String>()? else {
            return Err(serde::de::Error::custom(
                "expected one USDA root key, found an empty object",
            ));
        };
        let Some(root_kind) = food_kind_for_root(&root) else {
            return Err(serde::de::Error::custom(format!(
                "unsupported USDA root `{root}`; expected {ROOT_NAMES}"
            )));
        };

        map.next_value_seed(FoodListSeed {
            sender,
            state,
            root_kind,
        })?;

        if map.next_key::<IgnoredAny>()?.is_some() {
            return Err(serde::de::Error::custom(
                "expected exactly one USDA root key, found another entry",
            ));
        }

        Ok(())
    }
}

struct FoodListSeed<'a> {
    sender: &'a mpsc::Sender<PreparedFood>,
    state: &'a mut StreamState,
    root_kind: FoodKind,
}

impl<'de> DeserializeSeed<'de> for FoodListSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(FoodListVisitor {
            sender: self.sender,
            state: self.state,
            root_kind: self.root_kind,
        })
    }
}

struct FoodListVisitor<'a> {
    sender: &'a mpsc::Sender<PreparedFood>,
    state: &'a mut StreamState,
    root_kind: FoodKind,
}

impl<'de> Visitor<'de> for FoodListVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of USDA food objects")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let FoodListVisitor {
            sender,
            state,
            root_kind,
        } = self;
        let mut position = RecordPosition::first();

        loop {
            let raw = match sequence.next_element::<Box<RawValue>>() {
                Ok(Some(raw)) => raw,
                Ok(None) => return Ok(()),
                Err(source) => {
                    *state = StreamState::Record(position);
                    return Err(source);
                }
            };

            let food = match prepare_food(&raw, root_kind, position) {
                Ok(food) => food,
                Err(source) => {
                    *state = StreamState::Failure(ParseError::Record { position, source });
                    return Err(serde::de::Error::custom(PROCESSING_FAILED));
                }
            };

            match sender.blocking_send(food) {
                Ok(()) => {}
                Err(_) => {
                    *state = StreamState::Failure(ParseError::ConsumerDisconnected { position });
                    return Err(serde::de::Error::custom(PROCESSING_FAILED));
                }
            }

            position = match position.checked_next() {
                Some(next) => next,
                None => {
                    *state = StreamState::Failure(ParseError::RecordCountOverflow { position });
                    return Err(serde::de::Error::custom(PROCESSING_FAILED));
                }
            };
        }
    }
}

fn food_kind_for_root(root: &str) -> Option<FoodKind> {
    match root {
        FOUNDATION_ROOT => Some(FoodKind::Foundation),
        SR_LEGACY_ROOT => Some(FoodKind::SrLegacy),
        SURVEY_ROOT => Some(FoodKind::Survey),
        BRANDED_ROOT => Some(FoodKind::Branded),
        _ => None,
    }
}

fn prepare_food(
    raw: &RawValue,
    root_kind: FoodKind,
    position: RecordPosition,
) -> Result<PreparedFood, RecordError> {
    if !raw.get().trim_start().starts_with('{') {
        return Err(RecordError::NotObject);
    }

    let header: Header = serde_json::from_str(raw.get()).map_err(RecordError::Header)?;

    if header.data_type != root_kind {
        return Err(RecordError::KindMismatch {
            root: root_kind,
            data_type: header.data_type,
        });
    }

    let parsed = parsed_food_data(&header)?;
    let fdc_id = FdcId::from_json(header.fdc_id)?;
    let description = FoodDescription::from_json(header.description)?;
    let category = match header.data_type {
        FoodKind::Branded => header.branded_food_category.map(FoodCategory),
        FoodKind::Foundation | FoodKind::SrLegacy => header
            .food_category
            .and_then(|category| category.description)
            .map(FoodCategory),
        FoodKind::Survey => header
            .food_category
            .and_then(|category| category.description)
            .or_else(|| survey_category(raw.get()))
            .map(FoodCategory),
    };

    Ok(PreparedFood {
        fdc_id,
        kind: header.data_type,
        description,
        category,
        parsed,
        position,
    })
}

/// Parses a record's nutrition and serving data according to its food kind.
fn parsed_food_data(header: &Header) -> Result<ParsedFoodData, RecordError> {
    match header.data_type {
        FoodKind::Foundation => Ok(ParsedFoodData {
            nutrition: nutrient_profile(&header.food_nutrients)?,
            serving_sizes: foundation_servings(&header.food_portions),
        }),
        FoodKind::SrLegacy => Ok(ParsedFoodData {
            nutrition: nutrient_profile(&header.food_nutrients)?,
            serving_sizes: sr_legacy_servings(&header.food_portions),
        }),
        FoodKind::Survey => Ok(ParsedFoodData {
            nutrition: nutrient_profile(&header.food_nutrients)?,
            serving_sizes: survey_servings(&header.food_portions),
        }),
        FoodKind::Branded => branded_data(header),
    }
}

/// Parses the nutrition profile of a Foundation, SR Legacy, or Survey record.
///
/// These archives report nutrients for 100 g of edible food. A record whose
/// covered nutrients report nothing has no profile rather than an empty one.
fn nutrient_profile(nutrients: &[RawNutrient]) -> Result<Option<NutritionProfile>, RecordError> {
    let covered = collect_covered_nutrients(nutrients)?;
    let profile = NutritionProfile {
        per: NutritionBasis::Per100Grams,
        energy: select_energy(&covered)?,
        fat: Fat {
            total: mass_amount(&covered, MassConcept::TotalFat)?,
            saturated: mass_amount(&covered, MassConcept::SaturatedFat)?,
            monounsaturated: mass_amount(&covered, MassConcept::MonounsaturatedFat)?,
            polyunsaturated: mass_amount(&covered, MassConcept::PolyunsaturatedFat)?,
            trans: mass_amount(&covered, MassConcept::TransFat)?,
        },
        carbohydrates: Carbohydrates {
            including_fibre: mass_amount(&covered, MassConcept::CarbohydrateIncludingFibre)?,
            // USDA archives report no fibre-free carbohydrate measure.
            excluding_fibre: None,
            sugars: Sugars {
                total: mass_amount(&covered, MassConcept::TotalSugars)?,
                // No current USDA archive reports added sugars.
                added: None,
                lactose: mass_amount(&covered, MassConcept::Lactose)?,
            },
            starch: mass_amount(&covered, MassConcept::Starch)?,
        },
        fibre: mass_amount(&covered, MassConcept::Fibre)?,
        protein: mass_amount(&covered, MassConcept::Protein)?,
        sodium: mass_amount(&covered, MassConcept::Sodium)?,
        // USDA reports sodium only; salt is a European label value.
        salt: None,
        cholesterol: mass_amount(&covered, MassConcept::Cholesterol)?,
    };
    if reports_any_amount(&profile) {
        Ok(Some(profile))
    } else {
        Ok(None)
    }
}

/// Collects the covered nutrient entries of one record by nutrient identifier.
///
/// Entries for nutrients outside the coverage are ignored. An entry that
/// appears without an amount reports nothing. Duplicate covered nutrients and
/// unrepresentable amounts are rejected instead of being resolved silently.
fn collect_covered_nutrients(
    nutrients: &[RawNutrient],
) -> Result<BTreeMap<NutrientId, Option<CoveredNutrient>>, RecordError> {
    let mut covered = BTreeMap::new();
    for nutrient in nutrients {
        let Some(identity) = nutrient.nutrient.as_ref() else {
            // An entry without nutrient metadata cannot be identified.
            continue;
        };
        if !is_covered(identity.id) {
            continue;
        }
        let value = match nutrient.amount.as_ref() {
            Some(amount) => Some(CoveredNutrient {
                amount: decimal_from_json_number(amount).map_err(|source| {
                    RecordError::NutrientAmount {
                        nutrient_id: identity.id,
                        source,
                    }
                })?,
                unit: identity.unit.clone(),
            }),
            // A nutrient present without an amount reports no value.
            None => None,
        };
        if covered.insert(identity.id, value).is_some() {
            return Err(RecordError::DuplicateNutrient {
                nutrient_id: identity.id,
            });
        }
    }
    Ok(covered)
}

/// A covered nutrient's exact amount and its source unit name.
struct CoveredNutrient {
    amount: Decimal,
    unit: String,
}

/// Returns whether a nutrient identifier belongs to a covered concept.
fn is_covered(nutrient_id: NutrientId) -> bool {
    EnergyCandidate::SELECTION_ORDER
        .iter()
        .any(|candidate| candidate.nutrient_id() == nutrient_id)
        || MassConcept::CONCEPTS
            .iter()
            .any(|concept| concept.nutrient_ids().contains(&nutrient_id))
}

/// Selects the energy amount, following the project-defined candidate order.
///
/// Missing candidates fall through to the next one. A candidate reported in
/// the wrong unit is invalid data and fails the record rather than being
/// masked by the next candidate.
fn select_energy(
    covered: &BTreeMap<NutrientId, Option<CoveredNutrient>>,
) -> Result<Option<Energy>, RecordError> {
    for candidate in EnergyCandidate::SELECTION_ORDER {
        match covered.get(&candidate.nutrient_id()) {
            Some(Some(value)) => {
                if !unit_matches(&value.unit, candidate.unit()) {
                    return Err(RecordError::NutrientUnit {
                        nutrient_id: candidate.nutrient_id(),
                        unit: value.unit.clone(),
                    });
                }
                return Ok(Some(candidate.amount(value.amount)));
            }
            // A missing candidate, with or without an amount, falls through.
            Some(None) | None => continue,
        }
    }
    Ok(None)
}

/// Selects a covered mass nutrient's amount, following the concept's priority.
fn mass_amount(
    covered: &BTreeMap<NutrientId, Option<CoveredNutrient>>,
    concept: MassConcept,
) -> Result<Option<Mass>, RecordError> {
    for nutrient_id in concept.nutrient_ids() {
        match covered.get(nutrient_id) {
            Some(Some(value)) => {
                return Mass::from_parts(&value.unit, value.amount, None)
                    .map(Some)
                    .map_err(|_| RecordError::NutrientUnit {
                        nutrient_id: *nutrient_id,
                        unit: value.unit.clone(),
                    });
            }
            Some(None) | None => continue,
        }
    }
    Ok(None)
}

/// Energy candidates in the project-defined selection order.
///
/// Food-specific Atwater factors are preferred over general factors, then the
/// older unspecified energy values; the kJ variant is used only when no kcal
/// candidate is supplied. The order is Crumbs' engineering policy, not a
/// USDA-prescribed ranking, and the identifiers never reach the shared model.
#[derive(Debug, Clone, Copy)]
enum EnergyCandidate {
    /// Energy (Atwater Specific Factors).
    AtwaterSpecific,
    /// Energy (Atwater General Factors).
    AtwaterGeneral,
    /// Energy, the older unspecified kcal value.
    EnergyKcal,
    /// Energy, the older unspecified kJ value.
    EnergyKilojoules,
}

impl EnergyCandidate {
    /// Selection order: 2048, 2047, 1008, then 1062.
    const SELECTION_ORDER: [Self; 4] = [
        Self::AtwaterSpecific,
        Self::AtwaterGeneral,
        Self::EnergyKcal,
        Self::EnergyKilojoules,
    ];

    const fn nutrient_id(self) -> NutrientId {
        match self {
            Self::AtwaterSpecific => 2048,
            Self::AtwaterGeneral => 2047,
            Self::EnergyKcal => 1008,
            Self::EnergyKilojoules => 1062,
        }
    }

    const fn unit(self) -> &'static str {
        match self {
            Self::AtwaterSpecific | Self::AtwaterGeneral | Self::EnergyKcal => "kcal",
            Self::EnergyKilojoules => "kJ",
        }
    }

    /// Builds the energy amount in the candidate's unit.
    fn amount(self, value: Decimal) -> Energy {
        match self {
            Self::AtwaterSpecific | Self::AtwaterGeneral | Self::EnergyKcal => {
                Energy::Kilocalories(Kilocalories::new(value, None))
            }
            Self::EnergyKilojoules => Energy::Kilojoules(Kilojoules::new(value, None)),
        }
    }
}

/// Covered mass nutrients and the USDA nutrient identifiers that report them.
///
/// Identifier lists are in priority order; where the archives carry more than
/// one measure for a concept, the Foundation's standardized measure wins.
#[derive(Debug, Clone, Copy)]
enum MassConcept {
    /// Total lipid (fat).
    TotalFat,
    /// Fatty acids, total saturated.
    SaturatedFat,
    /// Fatty acids, total monounsaturated.
    MonounsaturatedFat,
    /// Fatty acids, total polyunsaturated.
    PolyunsaturatedFat,
    /// Fatty acids, total trans.
    TransFat,
    /// Carbohydrate by difference, which includes fibre.
    CarbohydrateIncludingFibre,
    /// Total sugars; Foundation's measure before the SR Legacy one.
    TotalSugars,
    /// Lactose.
    Lactose,
    /// Starch.
    Starch,
    /// Fibre, total dietary.
    Fibre,
    /// Protein.
    Protein,
    /// Sodium, Na.
    Sodium,
    /// Cholesterol.
    Cholesterol,
}

impl MassConcept {
    /// Every covered mass nutrient concept.
    const CONCEPTS: [Self; 13] = [
        Self::TotalFat,
        Self::SaturatedFat,
        Self::MonounsaturatedFat,
        Self::PolyunsaturatedFat,
        Self::TransFat,
        Self::CarbohydrateIncludingFibre,
        Self::TotalSugars,
        Self::Lactose,
        Self::Starch,
        Self::Fibre,
        Self::Protein,
        Self::Sodium,
        Self::Cholesterol,
    ];

    /// The USDA nutrient identifiers reporting this concept, in priority order.
    const fn nutrient_ids(self) -> &'static [NutrientId] {
        match self {
            Self::TotalFat => &[1004],
            Self::SaturatedFat => &[1258],
            Self::MonounsaturatedFat => &[1292],
            Self::PolyunsaturatedFat => &[1293],
            Self::TransFat => &[1257],
            Self::CarbohydrateIncludingFibre => &[1005],
            Self::TotalSugars => &[1063, 2000],
            Self::Lactose => &[1013],
            Self::Starch => &[1009],
            Self::Fibre => &[1079],
            Self::Protein => &[1003],
            Self::Sodium => &[1093],
            Self::Cholesterol => &[1253],
        }
    }
}

/// Collects the serving definitions of a Foundation record.
///
/// Foundation portions name themselves by count and measure unit, with the
/// modifier and the rare `portionDescription` appended. A portion without a
/// positive measured gram weight or a usable name defines no serving.
fn foundation_servings(portions: &[RawPortion]) -> Vec<ServingSize> {
    portions
        .iter()
        .filter_map(|portion| {
            let mass = portion.gram_mass()?;
            let name = foundation_portion_name(portion)?;
            Some(ServingSize {
                name: ServingName::new(name).ok()?,
                quantity: MeasuredQuantity::Mass(mass),
            })
        })
        .collect()
}

/// Builds a Foundation serving name from a portion's count, unit, and parts.
fn foundation_portion_name(portion: &RawPortion) -> Option<String> {
    let count = portion.measure_count()?.normalize();
    let unit = portion.measure_unit_name()?;
    let mut name = format!("{count} {unit}");
    for part in [
        portion.modifier.as_deref(),
        portion.portion_description.as_deref(),
    ] {
        if let Some(part) = trimmed_nonblank(part) {
            name.push_str(", ");
            name.push_str(part);
        }
    }
    Some(name)
}

/// Collects the serving definitions of an SR Legacy record.
///
/// SR Legacy portions carry their readable description in `modifier`; the
/// `measureUnit` of every portion is the placeholder `undetermined`.
fn sr_legacy_servings(portions: &[RawPortion]) -> Vec<ServingSize> {
    portions
        .iter()
        .filter_map(|portion| {
            let mass = portion.gram_mass()?;
            let name = sr_legacy_portion_name(portion)?;
            Some(ServingSize {
                name: ServingName::new(name).ok()?,
                quantity: MeasuredQuantity::Mass(mass),
            })
        })
        .collect()
}

/// Builds an SR Legacy serving name from a portion's count and description.
///
/// SR Legacy portions carry their measure in `modifier` — either a bare unit
/// word such as `oz` or `cup`, or a fuller description such as
/// `serving 1 roll with icing` — while the portion count is a separate
/// `value`. The count always precedes the measure so servings of different
/// counts stay distinct; the `measureUnit` of every portion is the
/// placeholder `undetermined`.
fn sr_legacy_portion_name(portion: &RawPortion) -> Option<String> {
    if let Some(description) = trimmed_nonblank(portion.portion_description.as_deref()) {
        return Some(description.to_owned());
    }
    let count = portion.measure_count()?.normalize();
    if let Some(modifier) = trimmed_nonblank(portion.modifier.as_deref()) {
        return Some(format!("{count} {modifier}"));
    }
    let unit = portion.measure_unit_name()?;
    Some(format!("{count} {unit}"))
}

/// Collects the serving definitions of a Survey record.
///
/// Survey portions are named by `portionDescription`, which already includes
/// the count, such as `1 cup`. Portions with a nonpositive gram weight, like
/// the source's `Quantity not specified`, define no serving.
fn survey_servings(portions: &[RawPortion]) -> Vec<ServingSize> {
    portions
        .iter()
        .filter_map(|portion| {
            let mass = portion.gram_mass()?;
            let name = trimmed_nonblank(portion.portion_description.as_deref())?;
            Some(ServingSize {
                name: ServingName::new(name).ok()?,
                quantity: MeasuredQuantity::Mass(mass),
            })
        })
        .collect()
}

/// Parses a Branded record into parsed food data.
///
/// Current archives publish standardized nutrient values for the record's
/// basis in `foodNutrients`, so those values take precedence per concept;
/// per-serving `labelNutrients` fill the gaps. The serving size with its unit
/// is the evidence for the profile basis, and without a positive serving size
/// in a supported unit there is no basis, and no nutrition is manufactured.
fn branded_data(header: &Header) -> Result<ParsedFoodData, RecordError> {
    let serving = branded_serving(header);
    let serving_sizes = match serving.as_ref() {
        Some(size) => vec![size.clone()],
        None => Vec::new(),
    };
    let nutrition = match serving.as_ref() {
        Some(serving) => branded_profile(header, serving)?,
        None => None,
    };
    Ok(ParsedFoodData {
        nutrition,
        serving_sizes,
    })
}

/// Builds a Branded serving definition from the record's serving fields.
///
/// The serving is named by `householdServingFullText`, or by the serving
/// itself when the record carries no household text. Serving sizes in units
/// other than grams or millilitres define no serving and no basis.
fn branded_serving(header: &Header) -> Option<ServingSize> {
    let size = header.serving_size.as_ref()?;
    let unit = header.serving_size_unit.as_deref()?;
    let size = measured_decimal_from_json_number(size).ok()?;
    let quantity = MeasuredQuantity::from_parts(unit, size).ok()?;
    let name = trimmed_nonblank(header.household_serving_full_text.as_deref()).unwrap_or("serving");
    Some(ServingSize {
        name: ServingName::new(name).ok()?,
        quantity,
    })
}

/// Builds a Branded nutrition profile on the serving's 100 g or 100 ml basis.
///
/// Standardized `foodNutrients` values are used per concept where the record
/// publishes them, including the energy selection order. Concepts without a
/// standardized value fall back to the record's per-serving label values in
/// label units: kcal for energy, g for fat, fatty acids, carbohydrate,
/// fibre, sugars, and protein, and mg for sodium and cholesterol. Each
/// fallback value is normalized by exact multiplication by 100 followed by
/// one fixed-point division, which rounds half to even at the maximum
/// representable precision; no other rounding is applied. Label
/// carbohydrates include fibre, following the US label.
fn branded_profile(
    header: &Header,
    serving: &ServingSize,
) -> Result<Option<NutritionProfile>, RecordError> {
    let basis = match serving.quantity {
        MeasuredQuantity::Mass(_) => NutritionBasis::Per100Grams,
        MeasuredQuantity::Volume(_) => NutritionBasis::Per100Millilitres,
    };
    let reference = serving.quantity.value();
    let standardized = collect_covered_nutrients(&header.food_nutrients)?;
    let labels = header.label_nutrients.as_ref();
    let label = |name: &'static str,
                 value: Option<&RawLabelValue>|
     -> Result<Option<Decimal>, RecordError> {
        let Some(value) = label_number(value) else {
            return Ok(None);
        };
        let per_serving =
            decimal_from_json_number(value).map_err(|source| RecordError::LabelNutrient {
                label: name,
                source,
            })?;
        per_serving
            .checked_mul(Decimal::ONE_HUNDRED)
            .and_then(|scaled| scaled.checked_div(reference))
            .ok_or(RecordError::LabelNutrient {
                label: name,
                source: MeasureError::ConversionOverflow,
            })
            .map(Some)
    };

    let calories = label(
        "calories",
        labels.and_then(|labels| labels.calories.as_ref()),
    )?;
    let fat_total = label("fat", labels.and_then(|labels| labels.fat.as_ref()))?;
    let saturated_fat = label(
        "saturatedFat",
        labels.and_then(|labels| labels.saturated_fat.as_ref()),
    )?;
    let trans_fat = label(
        "transFat",
        labels.and_then(|labels| labels.trans_fat.as_ref()),
    )?;
    let cholesterol = label(
        "cholesterol",
        labels.and_then(|labels| labels.cholesterol.as_ref()),
    )?;
    let sodium = label("sodium", labels.and_then(|labels| labels.sodium.as_ref()))?;
    let carbohydrates = label(
        "carbohydrates",
        labels.and_then(|labels| labels.carbohydrates.as_ref()),
    )?;
    let fibre = label("fiber", labels.and_then(|labels| labels.fiber.as_ref()))?;
    let sugars = label("sugars", labels.and_then(|labels| labels.sugars.as_ref()))?;
    let added_sugars = label(
        "addedSugar",
        labels.and_then(|labels| labels.added_sugar.as_ref()),
    )?;
    let protein = label("protein", labels.and_then(|labels| labels.protein.as_ref()))?;

    let profile = NutritionProfile {
        per: basis,
        energy: select_energy(&standardized)?
            .or(calories.map(|value| Energy::Kilocalories(Kilocalories::new(value, None)))),
        fat: Fat {
            total: mass_amount(&standardized, MassConcept::TotalFat)?.or(fat_total.map(grams)),
            saturated: mass_amount(&standardized, MassConcept::SaturatedFat)?
                .or(saturated_fat.map(grams)),
            // Labels carry no fatty-acid breakdown beyond saturated and trans.
            monounsaturated: mass_amount(&standardized, MassConcept::MonounsaturatedFat)?,
            polyunsaturated: mass_amount(&standardized, MassConcept::PolyunsaturatedFat)?,
            trans: mass_amount(&standardized, MassConcept::TransFat)?.or(trans_fat.map(grams)),
        },
        carbohydrates: Carbohydrates {
            including_fibre: mass_amount(&standardized, MassConcept::CarbohydrateIncludingFibre)?
                .or(carbohydrates.map(grams)),
            // Neither source reports a fibre-free carbohydrate measure.
            excluding_fibre: None,
            sugars: Sugars {
                total: mass_amount(&standardized, MassConcept::TotalSugars)?.or(sugars.map(grams)),
                // No current USDA archive reports added sugars; labels do.
                added: added_sugars.map(grams),
                // Labels carry no lactose.
                lactose: mass_amount(&standardized, MassConcept::Lactose)?,
            },
            // Labels carry no starch.
            starch: mass_amount(&standardized, MassConcept::Starch)?,
        },
        fibre: mass_amount(&standardized, MassConcept::Fibre)?.or(fibre.map(grams)),
        protein: mass_amount(&standardized, MassConcept::Protein)?.or(protein.map(grams)),
        sodium: mass_amount(&standardized, MassConcept::Sodium)?.or(sodium.map(milligrams)),
        // Neither source reports salt.
        salt: None,
        cholesterol: mass_amount(&standardized, MassConcept::Cholesterol)?
            .or(cholesterol.map(milligrams)),
    };
    if reports_any_amount(&profile) {
        Ok(Some(profile))
    } else {
        Ok(None)
    }
}

/// Builds a mass amount in grams from a normalized label amount.
fn grams(value: Decimal) -> Mass {
    Mass::Grams(Grams::new(value, None))
}

/// Builds a mass amount in milligrams from a normalized label amount.
fn milligrams(value: Decimal) -> Mass {
    Mass::Milligrams(Milligrams::new(value, None))
}

/// Returns a label nutrient's value, when the key and its value are present.
fn label_number(value: Option<&RawLabelValue>) -> Option<&JsonNumber> {
    value?.value.as_ref()
}

/// Returns the text trimmed, when trimming leaves characters.
fn trimmed_nonblank(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}

type RawFdcId = u64;
type RawFoodDescription = String;
type RawFoodCategory = String;
type NutrientId = u64;

#[derive(Debug, Deserialize)]
struct Header {
    #[serde(rename = "fdcId")]
    fdc_id: RawFdcId,
    description: RawFoodDescription,
    #[serde(rename = "dataType")]
    data_type: FoodKind,
    #[serde(default, rename = "foodCategory")]
    food_category: Option<FoodCategoryObject>,
    #[serde(default, rename = "brandedFoodCategory")]
    branded_food_category: Option<RawFoodCategory>,
    #[serde(default, rename = "foodNutrients")]
    food_nutrients: Vec<RawNutrient>,
    #[serde(default, rename = "foodPortions")]
    food_portions: Vec<RawPortion>,
    #[serde(default, rename = "servingSize")]
    serving_size: Option<JsonNumber>,
    #[serde(default, rename = "servingSizeUnit")]
    serving_size_unit: Option<String>,
    #[serde(default, rename = "householdServingFullText")]
    household_serving_full_text: Option<String>,
    #[serde(default, rename = "labelNutrients")]
    label_nutrients: Option<RawLabelNutrients>,
}

/// A `foodNutrients` entry with the covered nutrients' identity and amount.
#[derive(Debug, Deserialize)]
struct RawNutrient {
    #[serde(default)]
    nutrient: Option<RawNutrientIdentity>,
    #[serde(default)]
    amount: Option<JsonNumber>,
}

/// The `nutrient` object of a `foodNutrients` entry.
#[derive(Debug, Deserialize)]
struct RawNutrientIdentity {
    id: NutrientId,
    #[serde(default, rename = "unitName")]
    unit: String,
}

/// A `foodPortions` entry; archives fill different fields per food kind.
#[derive(Debug, Deserialize)]
struct RawPortion {
    #[serde(default)]
    value: Option<JsonNumber>,
    #[serde(default)]
    amount: Option<JsonNumber>,
    #[serde(default, rename = "measureUnit")]
    measure_unit: Option<RawMeasureUnit>,
    #[serde(default)]
    modifier: Option<String>,
    #[serde(default, rename = "portionDescription")]
    portion_description: Option<String>,
    #[serde(default, rename = "gramWeight")]
    gram_weight: Option<JsonNumber>,
}

impl RawPortion {
    /// Returns the portion's positive gram weight, when it defines one.
    ///
    /// Gram weights follow the measured-quantity rounding policy, so the
    /// Survey archive's binary-float spellings still define their servings.
    fn gram_mass(&self) -> Option<MeasuredMass> {
        let weight = measured_decimal_from_json_number(self.gram_weight.as_ref()?).ok()?;
        MeasuredMass::new(MassUnit::Gram, weight).ok()
    }

    /// Returns the portion count in its measure unit, preferring `value`.
    fn measure_count(&self) -> Option<Decimal> {
        measured_decimal_from_json_number(self.value.as_ref().or(self.amount.as_ref())?).ok()
    }

    /// Returns the measure unit name, when it names a real unit.
    fn measure_unit_name(&self) -> Option<String> {
        let name = trimmed_nonblank(self.measure_unit.as_ref()?.name.as_deref())?;
        (name != UNDETERMINED_UNIT).then(|| name.to_owned())
    }
}

/// The `measureUnit` object of a `foodPortions` entry.
#[derive(Debug, Deserialize)]
struct RawMeasureUnit {
    #[serde(default)]
    name: Option<String>,
}

/// The `labelNutrients` object of a Branded record.
#[derive(Debug, Deserialize)]
struct RawLabelNutrients {
    #[serde(default)]
    calories: Option<RawLabelValue>,
    #[serde(default)]
    fat: Option<RawLabelValue>,
    #[serde(default, rename = "saturatedFat")]
    saturated_fat: Option<RawLabelValue>,
    #[serde(default, rename = "transFat")]
    trans_fat: Option<RawLabelValue>,
    #[serde(default)]
    cholesterol: Option<RawLabelValue>,
    #[serde(default)]
    sodium: Option<RawLabelValue>,
    #[serde(default)]
    carbohydrates: Option<RawLabelValue>,
    #[serde(default, rename = "fiber")]
    fiber: Option<RawLabelValue>,
    #[serde(default)]
    sugars: Option<RawLabelValue>,
    #[serde(default, rename = "addedSugar")]
    added_sugar: Option<RawLabelValue>,
    #[serde(default)]
    protein: Option<RawLabelValue>,
}

/// One `labelNutrients` entry, holding its per-serving value.
#[derive(Debug, Deserialize)]
struct RawLabelValue {
    #[serde(default)]
    value: Option<JsonNumber>,
}

fn survey_category(raw: &str) -> Option<RawFoodCategory> {
    serde_json::from_str::<SurveyCategoryHeader>(raw)
        .ok()?
        .category?
        .0
        .description
}

#[derive(Deserialize)]
struct SurveyCategoryHeader {
    #[serde(default, rename = "wweiaFoodCategory")]
    category: Option<SurveyCategoryObject>,
}

struct SurveyCategoryObject(FoodCategoryObject);

impl<'de> Deserialize<'de> for SurveyCategoryObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer
            .deserialize_map(FoodCategoryVisitor("wweiaFoodCategoryDescription"))
            .map(Self)
    }
}

#[derive(Debug)]
struct FoodCategoryObject {
    description: Option<RawFoodCategory>,
}

impl<'de> Deserialize<'de> for FoodCategoryObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(FoodCategoryVisitor("description"))
    }
}

struct FoodCategoryVisitor(&'static str);

impl<'de> Visitor<'de> for FoodCategoryVisitor {
    type Value = FoodCategoryObject;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a food category object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut description = None;
        while let Some(key) = map.next_key::<String>()? {
            if key == self.0 {
                description = map.next_value::<Option<RawFoodCategory>>()?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(FoodCategoryObject { description })
    }
}

const USDA_SOURCE: Source = std::borrow::Cow::Borrowed("usda");

#[cfg(test)]
#[path = "../../unit-tests/import/usda.rs"]
mod tests;
