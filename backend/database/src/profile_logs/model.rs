use std::fmt;
use std::sync::LazyLock;

use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use crumbs_derive::{AliasTryNew, AliasValue, Display};
use regex::Regex;
use rust_decimal::{Decimal, dec};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::foods::{FoodDescription, FoodId};
use crate::nutrition_model::{
    ConsumedNutrition, FoodData, Grams, Kilograms, Litres, Mass, MeasuredQuantity, Micrograms,
    Milligrams, Millilitres, NutritionBasis, QuantityError, ServingName, ServingSize, Volume,
};
use crate::profiles::{BodyWeight, Patch, ProfileId};
use crate::text::NonBlankString;

use super::{ExerciseLogEntry, ExerciseLogPatch, ProfileLogError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ProfileLogId(i64);

impl ProfileLogId {
    pub fn new(value: i64) -> Result<Self, ProfileLogError> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(ProfileLogError::InvalidInput(
                "profile-log ID must be positive".to_owned(),
            ))
        }
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ProfileLogId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(i64::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogDay(NaiveDate);

impl LogDay {
    pub fn new(year: i32, month: u32, day: u32) -> Result<Self, ProfileLogError> {
        if !(1..=9999).contains(&year) {
            return Err(invalid_day());
        }
        NaiveDate::from_ymd_opt(year, month, day)
            .map(Self)
            .ok_or_else(invalid_day)
    }

    pub fn parse(value: &str) -> Result<Self, ProfileLogError> {
        static DATE_FORMAT: LazyLock<Result<Regex, regex::Error>> =
            LazyLock::new(|| Regex::new(r"\A[0-9]{4}-[0-9]{2}-[0-9]{2}\z"));

        let format = DATE_FORMAT
            .as_ref()
            .map_err(|error| ProfileLogError::storage(error.clone()))?;
        if !format.is_match(value) {
            return Err(invalid_day());
        }

        let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| invalid_day())?;
        if date.year() == 0 {
            return Err(invalid_day());
        }

        Ok(Self(date))
    }

    pub fn components(self) -> (i32, u32, u32) {
        (self.0.year(), self.0.month(), self.0.day())
    }
}

impl fmt::Display for LogDay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.0.year(),
            self.0.month(),
            self.0.day()
        )
    }
}

impl Serialize for LogDay {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for LogDay {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalTime(NaiveTime);

impl LocalTime {
    pub fn new(
        hour: u32,
        minute: u32,
        second: u32,
        millisecond: u32,
    ) -> Result<Self, ProfileLogError> {
        if millisecond > 999 {
            return Err(invalid_time());
        }
        NaiveTime::from_hms_milli_opt(hour, minute, second, millisecond)
            .map(Self)
            .ok_or_else(invalid_time)
    }

    pub fn parse(value: &str) -> Result<Self, ProfileLogError> {
        static TIME_FORMAT: LazyLock<Result<Regex, regex::Error>> =
            LazyLock::new(|| Regex::new(r"\A[0-9]{2}:[0-9]{2}(?::[0-5][0-9](?:\.[0-9]{1,3})?)?\z"));

        let format = TIME_FORMAT
            .as_ref()
            .map_err(|error| ProfileLogError::storage(error.clone()))?;
        if !format.is_match(value) {
            return Err(invalid_time());
        }

        let chrono_format = if value.len() == 5 {
            "%H:%M"
        } else {
            "%H:%M:%S%.f"
        };
        NaiveTime::parse_from_str(value, chrono_format)
            .map(Self)
            .map_err(|_| invalid_time())
    }

    pub fn components(self) -> (u32, u32, u32, u32) {
        (
            self.0.hour(),
            self.0.minute(),
            self.0.second(),
            self.0.nanosecond() / 1_000_000,
        )
    }
}

impl fmt::Display for LocalTime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (hour, minute, second, millisecond) = self.components();
        write!(
            formatter,
            "{hour:02}:{minute:02}:{second:02}.{millisecond:03}"
        )
    }
}

impl Serialize for LocalTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for LocalTime {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub struct Amount(Decimal);

impl Amount {
    pub fn new(value: Decimal) -> Result<Self, ProfileLogError> {
        if value.is_sign_positive() && !value.is_zero() {
            Ok(Self(value))
        } else {
            Err(invalid_food("amount must be positive"))
        }
    }

    pub fn parse(value: &str) -> Result<Self, ProfileLogError> {
        Decimal::from_str_exact(value)
            .map_err(|_| invalid_food("amount must be an exact positive decimal"))
            .and_then(Self::new)
    }

    pub const fn value(self) -> Decimal {
        self.0
    }
}

impl Serialize for Amount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Amount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MassUnit {
    Kilograms,
    Grams,
    Milligrams,
    Micrograms,
}

impl MassUnit {
    const fn token(self) -> &'static str {
        match self {
            Self::Kilograms => "kg",
            Self::Grams => "g",
            Self::Milligrams => "mg",
            Self::Micrograms => "µg",
        }
    }

    fn quantity(self) -> MeasuredQuantity {
        MeasuredQuantity::Mass(match self {
            Self::Kilograms => Mass::Kilograms(Kilograms::from(1)),
            Self::Grams => Mass::Grams(Grams::from(1)),
            Self::Milligrams => Mass::Milligrams(Milligrams::from(1)),
            Self::Micrograms => Mass::Micrograms(Micrograms::from(1)),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeUnit {
    Litres,
    Millilitres,
}

impl VolumeUnit {
    const fn token(self) -> &'static str {
        match self {
            Self::Litres => "l",
            Self::Millilitres => "ml",
        }
    }

    fn quantity(self) -> MeasuredQuantity {
        MeasuredQuantity::Volume(match self {
            Self::Litres => Volume::Litres(Litres::from(1)),
            Self::Millilitres => Volume::Millilitres(Millilitres::from(1)),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasurementUnit {
    Mass(MassUnit),
    Volume(VolumeUnit),
}

impl MeasurementUnit {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mass(unit) => unit.token(),
            Self::Volume(unit) => unit.token(),
        }
    }

    fn quantity(self) -> MeasuredQuantity {
        match self {
            Self::Mass(unit) => unit.quantity(),
            Self::Volume(unit) => unit.quantity(),
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "kg" => Some(Self::Mass(MassUnit::Kilograms)),
            "g" => Some(Self::Mass(MassUnit::Grams)),
            "mg" => Some(Self::Mass(MassUnit::Milligrams)),
            "µg" => Some(Self::Mass(MassUnit::Micrograms)),
            "l" => Some(Self::Volume(VolumeUnit::Litres)),
            "ml" => Some(Self::Volume(VolumeUnit::Millilitres)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SelectedServingName(String);

impl SelectedServingName {
    pub fn new(value: impl Into<String>) -> Result<Self, ProfileLogError> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(invalid_food("serving name must not be blank"));
        }
        if MeasurementUnit::parse(trimmed).is_some() {
            return Err(invalid_food(
                "serving name must not match a supported measurement unit",
            ));
        }
        if trimmed.len() == value.len() {
            Ok(Self(value))
        } else {
            Ok(Self(trimmed.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SelectedServingName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SelectedServingName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServingSelector {
    Unit(MeasurementUnit),
    Named(SelectedServingName),
}

impl ServingSelector {
    pub fn parse(value: &str) -> Result<Self, ProfileLogError> {
        let value = value.trim();
        match MeasurementUnit::parse(value) {
            Some(unit) => Ok(Self::Unit(unit)),
            None => Ok(Self::Named(SelectedServingName::new(value)?)),
        }
    }

    fn resolve(&self, servings: Vec<ServingSize>) -> Result<ServingSize, ProfileLogError> {
        match self {
            Self::Unit(unit) => ServingSize::new(unit.as_str().to_owned(), unit.quantity())
                .map_err(food_calculation_error),
            Self::Named(name) => {
                let mut matches = servings
                    .into_iter()
                    .filter(|serving| serving.name.trim() == name.as_str());
                let serving = matches.next().ok_or_else(|| {
                    ProfileLogError::InvalidInput(format!(
                        "serving '{}' is not available for this food",
                        name.as_str()
                    ))
                })?;
                if matches.next().is_some() {
                    return Err(ProfileLogError::InvalidInput(format!(
                        "serving '{}' is ambiguous for this food",
                        name.as_str()
                    )));
                }
                Ok(serving)
            }
        }
    }
}

impl fmt::Display for ServingSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unit(unit) => formatter.write_str(unit.as_str()),
            Self::Named(name) => fmt::Display::fmt(name, formatter),
        }
    }
}

impl Serialize for ServingSelector {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ServingSelector {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename = "Fields")]
pub struct FoodLogInput {
    food_id: FoodId,
    amount: Amount,
    serving_size: ServingSelector,
}

impl FoodLogInput {
    pub fn new(
        food_id: FoodId,
        amount: Amount,
        serving_size: ServingSelector,
    ) -> Result<Self, ProfileLogError> {
        validate_food_id(food_id)?;
        Ok(Self {
            food_id,
            amount,
            serving_size,
        })
    }

    pub const fn food_id(&self) -> FoodId {
        self.food_id
    }

    pub const fn amount(&self) -> Amount {
        self.amount
    }

    pub fn serving_size(&self) -> &ServingSelector {
        &self.serving_size
    }
}

impl<'de> Deserialize<'de> for FoodLogInput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            food_id: i64,
            amount: Amount,
            serving_size: ServingSelector,
        }

        let fields = Fields::deserialize(deserializer)?;
        Self::new(FoodId(fields.food_id), fields.amount, fields.serving_size)
            .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoodInputSnapshot {
    amount: Amount,
    serving_size: ServingSnapshot,
}

pub type ServingSnapshot = ServingSize;

impl FoodInputSnapshot {
    pub fn new(amount: Amount, serving_size: ServingSnapshot) -> Result<Self, ProfileLogError> {
        if serving_size.name.trim().is_empty() {
            return Err(invalid_food("serving snapshot name must not be blank"));
        }
        Ok(Self {
            amount,
            serving_size,
        })
    }

    pub const fn amount(&self) -> Amount {
        self.amount
    }

    pub const fn serving_size(&self) -> &ServingSnapshot {
        &self.serving_size
    }
}

impl<'de> Deserialize<'de> for FoodInputSnapshot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ServingFields {
            name: ServingName,
            quantity: MeasuredQuantity,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            amount: Amount,
            serving_size: ServingFields,
        }

        let fields = Fields::deserialize(deserializer)?;
        let serving = ServingSize::new(fields.serving_size.name, fields.serving_size.quantity)
            .map_err(D::Error::custom)?;
        Self::new(fields.amount, serving).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoodLogEntry {
    food_id: Option<FoodId>,
    quantity: MeasuredQuantity,
    nutrition: ConsumedNutrition,
    description: FoodDescription,
    input: Option<FoodInputSnapshot>,
}

impl FoodLogEntry {
    pub fn new(
        food_id: Option<FoodId>,
        quantity: MeasuredQuantity,
        nutrition: ConsumedNutrition,
        description: FoodDescription,
        input: Option<FoodInputSnapshot>,
    ) -> Result<Self, ProfileLogError> {
        if let Some(food_id) = food_id {
            validate_food_id(food_id)?;
        }
        if !quantity.is_positive() {
            return Err(invalid_food("consumed quantity must be positive"));
        }
        if description.trim().is_empty() {
            return Err(invalid_food("food description must not be blank"));
        }
        Ok(Self {
            food_id,
            quantity,
            nutrition,
            description,
            input,
        })
    }

    pub const fn food_id(&self) -> Option<FoodId> {
        self.food_id
    }

    pub const fn quantity(&self) -> MeasuredQuantity {
        self.quantity
    }

    pub const fn nutrition(&self) -> &ConsumedNutrition {
        &self.nutrition
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub const fn input(&self) -> Option<&FoodInputSnapshot> {
        self.input.as_ref()
    }

    pub(super) fn from_food(
        input: &FoodLogInput,
        description: FoodDescription,
        food_data: FoodData,
    ) -> Result<Self, ProfileLogError> {
        let nutrition = food_data.nutrition.as_ref().ok_or_else(missing_calories)?;
        let source_energy = nutrition.energy.ok_or_else(missing_calories)?;

        let serving = input.serving_size().resolve(food_data.serving_sizes)?;
        let quantity = serving.quantity() * input.amount().value();
        if quantity.value().is_zero() {
            return Err(unsupported_precision("consumed quantity"));
        }

        let factor = nutrition_factor(nutrition.per, quantity)?;
        let nutrition = ConsumedNutrition::from_source(source_energy, &nutrition.nutrients, factor)
            .map_err(food_calculation_error)?;

        let input_snapshot = FoodInputSnapshot::new(input.amount(), serving)?;
        Self::new(
            Some(input.food_id()),
            quantity,
            nutrition,
            description,
            Some(input_snapshot),
        )
    }
}

impl<'de> Deserialize<'de> for FoodLogEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            #[serde(default)]
            food_id: Option<i64>,
            quantity: MeasuredQuantity,
            nutrition: ConsumedNutrition,
            description: FoodDescription,
            #[serde(default)]
            input: Option<FoodInputSnapshot>,
        }

        let fields = Fields::deserialize(deserializer)?;
        Self::new(
            fields.food_id.map(FoodId),
            fields.quantity,
            fields.nutrition,
            fields.description,
            fields.input,
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoodLogPatch {
    food_id: Patch<FoodId>,
    amount: Patch<Amount>,
    serving_size: Patch<ServingSelector>,
}

impl FoodLogPatch {
    pub fn new(
        food_id: Patch<FoodId>,
        amount: Patch<Amount>,
        serving_size: Patch<ServingSelector>,
    ) -> Result<Self, ProfileLogError> {
        if let Patch::Set(food_id) = &food_id {
            validate_food_id(*food_id)?;
        }
        Ok(Self {
            food_id,
            amount,
            serving_size,
        })
    }

    pub const fn food_id(&self) -> &Patch<FoodId> {
        &self.food_id
    }

    pub const fn amount(&self) -> &Patch<Amount> {
        &self.amount
    }

    pub const fn serving_size(&self) -> &Patch<ServingSelector> {
        &self.serving_size
    }

    pub const fn has_changes(&self) -> bool {
        !matches!(&self.food_id, Patch::Unchanged)
            || !matches!(&self.amount, Patch::Unchanged)
            || !matches!(&self.serving_size, Patch::Unchanged)
    }

    pub(super) fn apply(&self, current: &FoodLogEntry) -> Result<FoodLogInput, ProfileLogError> {
        let food_id = match &self.food_id {
            Patch::Unchanged => current.food_id(),
            Patch::Set(food_id) => Some(*food_id),
        }
        .ok_or_else(|| {
            ProfileLogError::InvalidInput(
                "food_id is required to refresh this food entry".to_owned(),
            )
        })?;

        let amount = match &self.amount {
            Patch::Unchanged => current.input().map(FoodInputSnapshot::amount),
            Patch::Set(amount) => Some(*amount),
        }
        .ok_or_else(|| {
            ProfileLogError::InvalidInput(
                "amount is required to refresh this food entry".to_owned(),
            )
        })?;

        let serving_size = match &self.serving_size {
            Patch::Unchanged => current
                .input()
                .map(FoodInputSnapshot::serving_size)
                .map(|serving| ServingSelector::parse(&serving.name))
                .transpose()?,
            Patch::Set(serving_size) => Some(serving_size.clone()),
        }
        .ok_or_else(|| {
            ProfileLogError::InvalidInput(
                "serving_size is required to refresh this food entry".to_owned(),
            )
        })?;

        Ok(FoodLogInput {
            food_id,
            amount,
            serving_size,
        })
    }
}

impl<'de> Deserialize<'de> for FoodLogPatch {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            #[serde(default)]
            food_id: Patch<i64>,
            #[serde(default)]
            amount: Patch<Amount>,
            #[serde(default)]
            serving_size: Patch<ServingSelector>,
        }

        let fields = Fields::deserialize(deserializer)?;
        let food_id = match fields.food_id {
            Patch::Unchanged => Patch::Unchanged,
            Patch::Set(food_id) => Patch::Set(FoodId(food_id)),
        };
        Self::new(food_id, fields.amount, fields.serving_size).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightMeasurement {
    pub value: BodyWeight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BiometricType {
    Weight,
}

impl BiometricType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Weight => "weight",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BiometricEntry {
    Weight(WeightMeasurement),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BiometricLogEntry {
    pub measurement: BiometricEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, AliasTryNew, AliasValue)]
#[serde(transparent)]
pub struct JournalText(NonBlankString);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalLogEntry {
    pub text: JournalText,
}

#[allow(
    clippy::large_enum_variant,
    reason = "Log payloads remain inline; log pages are bounded to 100 entries."
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileLogData {
    Biometric(BiometricLogEntry),
    Food(FoodLogEntry),
    Journal(JournalLogEntry),
    Exercise(ExerciseLogEntry),
}

impl ProfileLogData {
    pub(super) fn is_weight(&self) -> bool {
        matches!(
            self,
            Self::Biometric(BiometricLogEntry {
                measurement: BiometricEntry::Weight(_)
            })
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LogInput {
    Biometric(BiometricLogEntry),
    Food(FoodLogInput),
    Journal(JournalLogEntry),
    Exercise(ExerciseLogEntry),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateLog {
    pub day: LogDay,
    pub time: Option<LocalTime>,
    pub entry: LogInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LogEntryPatch {
    Biometric(BiometricLogEntry),
    Food(FoodLogPatch),
    Journal(JournalLogEntry),
    Exercise(ExerciseLogPatch),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchLog {
    #[serde(default)]
    pub day: Patch<LogDay>,
    #[serde(default)]
    pub time: Patch<LocalTime>,
    #[serde(default)]
    pub entry: Patch<LogEntryPatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogEntryType {
    Food,
    Biometric,
    Journal,
    Exercise,
}

impl LogEntryType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Food => "food",
            Self::Biometric => "biometric",
            Self::Journal => "journal",
            Self::Exercise => "exercise",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogFilter {
    from: Option<LogDay>,
    to: Option<LogDay>,
    entry_type: Option<LogEntryType>,
    subtype: Option<BiometricType>,
    last_id: Option<ProfileLogId>,
}

impl LogFilter {
    pub fn new(
        from: Option<LogDay>,
        to: Option<LogDay>,
        entry_type: Option<LogEntryType>,
        subtype: Option<BiometricType>,
        last_id: Option<ProfileLogId>,
    ) -> Result<Self, ProfileLogError> {
        if from.zip(to).is_some_and(|(from, to)| from > to) {
            return Err(ProfileLogError::InvalidInput(
                "from day must not be later than to day".to_owned(),
            ));
        }
        if subtype.is_some() && entry_type != Some(LogEntryType::Biometric) {
            return Err(ProfileLogError::InvalidInput(
                "subtype requires entry_type biometric".to_owned(),
            ));
        }
        Ok(Self {
            from,
            to,
            entry_type,
            subtype,
            last_id,
        })
    }

    pub const fn from(&self) -> Option<LogDay> {
        self.from
    }

    pub const fn to(&self) -> Option<LogDay> {
        self.to
    }

    pub const fn entry_type(&self) -> Option<LogEntryType> {
        self.entry_type
    }

    pub const fn subtype(&self) -> Option<BiometricType> {
        self.subtype
    }

    pub const fn last_id(&self) -> Option<ProfileLogId> {
        self.last_id
    }
}

impl<'de> Deserialize<'de> for LogFilter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            #[serde(default)]
            from: Option<LogDay>,
            #[serde(default)]
            to: Option<LogDay>,
            #[serde(default)]
            entry_type: Option<LogEntryType>,
            #[serde(default)]
            subtype: Option<BiometricType>,
            #[serde(default)]
            last_id: Option<ProfileLogId>,
        }

        let fields = Fields::deserialize(deserializer)?;
        Self::new(
            fields.from,
            fields.to,
            fields.entry_type,
            fields.subtype,
            fields.last_id,
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileLog {
    pub id: ProfileLogId,
    pub profile_id: ProfileId,
    pub day: LogDay,
    pub time: LocalTime,
    pub data: ProfileLogData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogPage {
    pub items: Vec<ProfileLog>,
    pub last_id: Option<ProfileLogId>,
}

fn invalid_day() -> ProfileLogError {
    ProfileLogError::InvalidInput("day must be a valid YYYY-MM-DD date".to_owned())
}

fn invalid_time() -> ProfileLogError {
    ProfileLogError::InvalidInput("time must be a valid local HH:MM[:SS[.mmm]] value".to_owned())
}

fn validate_food_id(food_id: FoodId) -> Result<(), ProfileLogError> {
    if food_id.0 > 0 {
        Ok(())
    } else {
        Err(invalid_food("food ID must be positive"))
    }
}

fn invalid_food(message: &str) -> ProfileLogError {
    ProfileLogError::InvalidInput(message.to_owned())
}

fn nutrition_factor(
    basis: NutritionBasis,
    quantity: MeasuredQuantity,
) -> Result<Decimal, ProfileLogError> {
    let normalized = match (basis, quantity) {
        (NutritionBasis::Per100Grams, MeasuredQuantity::Mass(mass)) => mass.to_grams().value(),
        (NutritionBasis::Per100Millilitres, MeasuredQuantity::Volume(volume)) => {
            volume.to_millilitres().value()
        }
        _ => {
            return Err(ProfileLogError::InvalidInput(
                "serving unit must match the food nutrition basis".to_owned(),
            ));
        }
    };
    if normalized.is_zero() {
        return Err(unsupported_precision("normalized quantity"));
    }
    let factor = normalized / dec!(100);
    if factor.is_zero() {
        return Err(unsupported_precision("nutrition scale"));
    }
    Ok(factor)
}

fn missing_calories() -> ProfileLogError {
    ProfileLogError::InvalidInput(
        "food has no usable calorie data; edit the food's nutrition or create a food with calories, then retry".to_owned(),
    )
}

fn unsupported_precision(value: &str) -> ProfileLogError {
    ProfileLogError::InvalidInput(format!(
        "{value} is below the supported fixed-point precision"
    ))
}

fn food_calculation_error(error: QuantityError) -> ProfileLogError {
    ProfileLogError::InvalidInput(format!(
        "food quantity or nutrition cannot be represented at supported fixed-point precision: {error}"
    ))
}
