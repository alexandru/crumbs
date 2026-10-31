//! Streaming parser for the official USDA FoodData Central JSON archives.
//!
//! The archives wrap a single array under one root key. This module validates
//! the root and each food header while forwarding every record as a
//! [`PreparedFood`] without collecting the array or a `serde_json::Value`.

use std::fmt;
use std::io::Read;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;

use serde::Deserialize;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Deserializer;
use serde_json::value::RawValue;
use tokio::sync::mpsc;

use crate::{Food, Source};

/// A validated food record ready for persistence.
///
/// `raw` holds the original JSON object exactly as it appeared in the archive.
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
    /// Original record object, including every detail field and number spelling.
    pub(crate) raw: Box<RawValue>,
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
            source_data: self.raw,
        }
    }
}

/// A positive FoodData Central identifier represented as decimal text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FdcId(String);

impl FdcId {
    /// Returns the identifier as text.
    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

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
    /// Returns the description as text.
    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

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

impl FoodCategory {
    /// Returns the category as text.
    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FoodCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One-based position of a record within the root food array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RecordPosition(NonZeroUsize);

impl RecordPosition {
    /// Returns the position as a one-based number.
    #[cfg(test)]
    pub(crate) const fn get(self) -> usize {
        self.0.get()
    }

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
    /// A food record is invalid. `source` identifies the field problem.
    #[error("invalid USDA FoodData Central record {position}: {source}")]
    Record {
        /// One-based record position.
        position: RecordPosition,
        /// Record-level validation failure.
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

/// Details of an invalid food record.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RecordError {
    /// The record is not a valid header object. Serde's line and column are
    /// relative to the start of the record object.
    #[error("malformed food header: {0}")]
    Header(#[from] serde_json::Error),
    /// The record JSON is malformed. Serde's line and column are relative to
    /// the archive document.
    #[error("malformed record JSON: {0}")]
    Syntax(#[source] serde_json::Error),
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
}

/// Streams food records from `reader` into `sender`.
///
/// The input must be a single top-level object with exactly one recognized
/// array: `FoundationFoods`, `SRLegacyFoods`, `SurveyFoods`, or `BrandedFoods`.
/// The parser validates each header, forwards the original record, and never
/// collects the array.
///
/// The function uses [`mpsc::Sender::blocking_send`], so it must run on a
/// blocking thread, such as one created by [`tokio::task::spawn_blocking`],
/// rather than on an async executor thread.
///
/// # Errors
///
/// Returns [`ParseError::Document`] for invalid root framing or trailing
/// content, [`ParseError::Record`] for an invalid record, and
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
        (Ok(()), StreamState::Ready | StreamState::Syntax(_)) => deserializer
            .end()
            .map_err(|source| ParseError::Document { source }),
        (Err(source), StreamState::Syntax(position)) => Err(ParseError::Record {
            position,
            source: RecordError::Syntax(source),
        }),
        (Err(source), StreamState::Ready) => Err(ParseError::Document { source }),
    }
}

const FOUNDATION_ROOT: &str = "FoundationFoods";
const SR_LEGACY_ROOT: &str = "SRLegacyFoods";
const SURVEY_ROOT: &str = "SurveyFoods";
const BRANDED_ROOT: &str = "BrandedFoods";
const ROOT_NAMES: &str = "FoundationFoods, SRLegacyFoods, SurveyFoods, or BrandedFoods";
const PROCESSING_FAILED: &str = "usda fooddata central processing failed";

enum StreamState {
    Ready,
    Failure(ParseError),
    Syntax(RecordPosition),
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
                    *state = StreamState::Syntax(position);
                    return Err(source);
                }
            };

            let food = match prepare_food(raw, root_kind, position) {
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
    raw: Box<RawValue>,
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
        raw,
        position,
    })
}

type RawFdcId = u64;
type RawFoodDescription = String;
type RawFoodCategory = String;

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
