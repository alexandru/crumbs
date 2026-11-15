use std::fmt;
use std::io::Read;
use std::num::NonZeroU64;
use std::num::NonZeroUsize;

use crate::{DiagnosticField, DiagnosticReason};
use serde::Deserialize;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Deserializer;
use serde_json::value::RawValue;
use tokio::sync::mpsc;

#[path = "usda_nutrition.rs"]
mod usda_nutrition;

use crumbs_database::foods::{Dataset, Food, Source, SourceFoodId};
use crumbs_database::nutrition_model::FoodData;

#[derive(Debug)]
pub(crate) struct PreparedFood {
    pub(crate) fdc_id: FdcId,
    pub(crate) kind: FoodKind,
    pub(crate) description: FoodDescription,
    pub(crate) category: Option<FoodCategory>,
    pub(crate) parsed_data: FoodData,
    pub(crate) sanitizations: Vec<usda_nutrition::Sanitization>,
    pub(crate) position: RecordPosition,
}

impl PreparedFood {
    pub(crate) fn into_food(self) -> Food {
        Food {
            source: USDA_SOURCE,
            source_food_id: self.fdc_id.0,
            dataset: self.kind.dataset(),
            description: self.description.0,
            category: self.category.map(|category| category.0),
            notes: None,
            parsed_data: self.parsed_data,
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum FoodKind {
    #[serde(rename = "Foundation")]
    Foundation,
    #[serde(rename = "SR Legacy")]
    SrLegacy,
    #[serde(rename = "Survey (FNDDS)")]
    Survey,
    #[serde(rename = "Branded")]
    Branded,
}

impl FoodKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Foundation => "Foundation",
            Self::SrLegacy => "SR Legacy",
            Self::Survey => "Survey (FNDDS)",
            Self::Branded => "Branded",
        }
    }

    pub(crate) const fn dataset(self) -> Dataset {
        match self {
            Self::Foundation | Self::SrLegacy | Self::Survey => Dataset::Common,
            Self::Branded => Dataset::Branded,
        }
    }
}

impl fmt::Display for FoodKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoodCategory(String);

impl fmt::Display for FoodCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecordPosition(NonZeroUsize);

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

    pub const fn get(self) -> usize {
        self.0.get()
    }
}

impl fmt::Display for RecordPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ParseError {
    #[error("invalid USDA FoodData Central document: {source}")]
    Document {
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to read USDA FoodData Central document: {source}")]
    Read {
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid USDA FoodData Central record {position}: {source}")]
    Record {
        position: RecordPosition,
        #[source]
        source: RecordError,
    },
    #[error("consumer disconnected at USDA FoodData Central record {position}")]
    ConsumerDisconnected { position: RecordPosition },
    #[error("record count overflowed at USDA FoodData Central record {position}")]
    RecordCountOverflow { position: RecordPosition },
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

#[derive(Debug, thiserror::Error)]
pub(crate) enum RecordError {
    #[error("malformed food header: {0}")]
    Header(#[from] serde_json::Error),
    #[error("malformed record JSON: {0}")]
    Syntax(#[source] serde_json::Error),
    #[error("failed to read record JSON: {0}")]
    Read(#[source] serde_json::Error),
    #[error("food record must be a JSON object")]
    NotObject,
    #[error("fdcId must be positive")]
    ZeroFdcId,
    #[error("description must not be blank")]
    BlankDescription,
    #[error("dataType {data_type} does not match the {root} root")]
    KindMismatch { root: FoodKind, data_type: FoodKind },
    #[error("invalid food nutrition or serving data: {0}")]
    Nutrition(#[source] usda_nutrition::NutritionError),
}

impl RecordError {
    fn field(&self) -> Option<DiagnosticField> {
        match self {
            Self::Nutrition(source) => source.field(),
            _ => None,
        }
    }
}

pub(crate) struct RejectedFood {
    pub(crate) position: RecordPosition,
    pub(crate) source_food_id: Option<SourceFoodId>,
    pub(crate) field: Option<DiagnosticField>,
    pub(crate) reason: DiagnosticReason,
}

pub(crate) type StreamItem = Result<PreparedFood, RejectedFood>;

pub(crate) fn stream_foods(
    reader: impl Read,
    sender: &mpsc::Sender<StreamItem>,
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

enum StreamState {
    Ready,
    Failure(ParseError),
    Record(RecordPosition),
}

struct RootSeed<'a> {
    sender: &'a mpsc::Sender<StreamItem>,
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
    sender: &'a mpsc::Sender<StreamItem>,
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
    sender: &'a mpsc::Sender<StreamItem>,
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
    sender: &'a mpsc::Sender<StreamItem>,
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

            let item = match prepare_food(&raw, root_kind, position) {
                Ok(food) => Ok(food),
                Err(source) => Err(RejectedFood {
                    position,
                    source_food_id: raw_fdc_id(raw.get()),
                    field: source.field(),
                    reason: source.to_string(),
                }),
            };

            match sender.blocking_send(item) {
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

    let fdc_id = FdcId::from_json(header.fdc_id)?;
    let description = FoodDescription::from_json(header.description)?;
    let raw_categories: RawCategories =
        serde_json::from_str(raw.get()).map_err(RecordError::Header)?;
    let mut category_sanitizations = Vec::new();
    let category = match header.data_type {
        FoodKind::Branded => raw_categories.branded_food_category.parse(
            "brandedFoodCategory",
            None,
            &mut category_sanitizations,
        ),
        FoodKind::Foundation | FoodKind::SrLegacy | FoodKind::Survey => {
            let category = raw_categories.food_category.parse(
                "foodCategory",
                Some("description"),
                &mut category_sanitizations,
            );
            if header.data_type == FoodKind::Survey {
                let survey = raw_categories.wweia_food_category.parse(
                    "wweiaFoodCategory",
                    Some("wweiaFoodCategoryDescription"),
                    &mut category_sanitizations,
                );
                category.or(survey)
            } else {
                category
            }
        }
    }
    .map(FoodCategory);
    let parsed_data = usda_nutrition::parse_food_data(raw.get(), header.data_type)
        .map_err(RecordError::Nutrition)?;
    let mut sanitizations = parsed_data.sanitizations;
    sanitizations.extend(category_sanitizations);

    Ok(PreparedFood {
        fdc_id,
        kind: header.data_type,
        description,
        category,
        parsed_data: parsed_data.food_data,
        sanitizations,
        position,
    })
}

fn raw_fdc_id(raw: &str) -> Option<SourceFoodId> {
    #[derive(Deserialize)]
    struct Id {
        #[serde(default, rename = "fdcId")]
        id: Option<Box<RawValue>>,
    }

    let id = serde_json::from_str::<Id>(raw).ok()?.id?;
    let raw_id = id.get().trim();
    if raw_id.starts_with('"') {
        serde_json::from_str::<String>(raw_id).ok()
    } else {
        serde_json::from_str::<serde_json::Number>(raw_id)
            .ok()
            .map(|_| raw_id.to_owned())
    }
}

type RawFdcId = u64;
type RawFoodDescription = String;

#[derive(Default)]
struct RawCategories {
    food_category: RawCategoryField,
    branded_food_category: RawCategoryField,
    wweia_food_category: RawCategoryField,
}

#[derive(Default)]
enum RawCategoryField {
    #[default]
    Absent,
    Value(Option<Box<RawValue>>),
    Duplicate,
}

impl RawCategoryField {
    fn parse(
        self,
        field: &'static str,
        description: Option<&'static str>,
        sanitizations: &mut Vec<usda_nutrition::Sanitization>,
    ) -> Option<String> {
        let parsed = match self {
            Self::Absent | Self::Value(None) => return None,
            Self::Value(Some(raw)) => match description {
                Some(key) => serde::Deserializer::deserialize_map(
                    &mut Deserializer::from_str(raw.get()),
                    FoodCategoryVisitor(key),
                ),
                None => serde_json::from_str(raw.get()),
            }
            .map_err(|error| format!("invalid optional category was omitted: {error}")),
            Self::Duplicate => Err("duplicate optional category was omitted".to_owned()),
        };
        match parsed {
            Ok(value) => value,
            Err(reason) => {
                sanitizations.push(usda_nutrition::Sanitization {
                    field: field.to_owned(),
                    reason,
                });
                None
            }
        }
    }
}

impl<'de> Deserialize<'de> for RawCategories {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct RawCategoriesVisitor;

        impl<'de> Visitor<'de> for RawCategoriesVisitor {
            type Value = RawCategories;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a USDA food record object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut categories = RawCategories::default();
                while let Some(key) = map.next_key::<String>()? {
                    let field = match key.as_str() {
                        "foodCategory" => &mut categories.food_category,
                        "brandedFoodCategory" => &mut categories.branded_food_category,
                        "wweiaFoodCategory" => &mut categories.wweia_food_category,
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                            continue;
                        }
                    };
                    *field = match field {
                        RawCategoryField::Absent => RawCategoryField::Value(map.next_value()?),
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                            RawCategoryField::Duplicate
                        }
                    };
                }
                Ok(categories)
            }
        }

        deserializer.deserialize_map(RawCategoriesVisitor)
    }
}

#[derive(Debug, Deserialize)]
struct Header {
    #[serde(rename = "fdcId")]
    fdc_id: RawFdcId,
    description: RawFoodDescription,
    #[serde(rename = "dataType")]
    data_type: FoodKind,
}

struct FoodCategoryVisitor(&'static str);

impl<'de> Visitor<'de> for FoodCategoryVisitor {
    type Value = Option<String>;

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
                description = map.next_value::<Option<String>>()?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(description)
    }
}

const USDA_SOURCE: Source = std::borrow::Cow::Borrowed("usda");

#[cfg(test)]
#[path = "../unit-tests/usda.rs"]
mod tests;
