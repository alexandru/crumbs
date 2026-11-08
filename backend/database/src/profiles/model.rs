use chrono::{Datelike, NaiveDate};
use chrono_tz::Tz;
use rust_decimal::Decimal;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use unicase::UniCase;
use unicode_normalization::UnicodeNormalization;

use crate::clock::UnixMillis;
use crate::nutrition_model::{Centimetres, Kilocalories, Kilograms, QuantityError};

use super::ProfileError;

/// A positive, stable profile identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ProfileId(i64);

impl ProfileId {
    /// Creates a profile identifier from a positive integer.
    ///
    /// # Errors
    ///
    /// Returns [ProfileError::InvalidInput] if value is not positive.
    pub fn new(value: i64) -> Result<Self, ProfileError> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(ProfileError::InvalidInput("invalid profile ID".to_owned()))
        }
    }

    /// Returns the integer value of this identifier.
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ProfileId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = i64::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// A nonblank profile name, retaining its trimmed display spelling.
///
/// Equality uses Unicode canonical caseless matching and preserves accents.
#[derive(Debug, Clone)]
pub struct ProfileName {
    display: String,
    key: String,
}

impl ProfileName {
    /// Creates a profile name and its account-scoped uniqueness key.
    ///
    /// # Errors
    ///
    /// Returns [ProfileError::InvalidInput] if trimming leaves the name blank.
    pub fn new(value: impl AsRef<str>) -> Result<Self, ProfileError> {
        let display = value.as_ref().trim();
        if display.is_empty() {
            return Err(ProfileError::InvalidInput(
                "profile name must not be blank".to_owned(),
            ));
        }
        let decomposed = display.nfd().collect::<String>();
        let key = UniCase::new(decomposed).to_folded_case().nfd().collect();
        Ok(Self {
            display: display.to_owned(),
            key,
        })
    }

    /// Returns the trimmed display spelling.
    pub fn as_str(&self) -> &str {
        &self.display
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }
}

impl PartialEq for ProfileName {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for ProfileName {}

impl Hash for ProfileName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

impl Serialize for ProfileName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProfileName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Precision retained for a partial or complete birth date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BirthDatePrecision {
    /// The year is known.
    Year,
    /// The year and month are known.
    YearMonth,
    /// The complete calendar date is known.
    FullDate,
}

/// A validated birth date whose original precision is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BirthDate {
    date: NaiveDate,
    precision: BirthDatePrecision,
}

impl BirthDate {
    /// Creates a birth date with year precision.
    ///
    /// Supported years are 1..=9999.
    ///
    /// # Errors
    ///
    /// Returns [ProfileError::InvalidInput] if the year is outside that range.
    pub fn year(year: i32) -> Result<Self, ProfileError> {
        Self::from_parts(year, 1, 1, BirthDatePrecision::Year)
    }

    /// Creates a birth date with year-month precision.
    ///
    /// Supported years are 1..=9999.
    ///
    /// # Errors
    ///
    /// Returns [ProfileError::InvalidInput] if the year is outside that range
    /// or the month is invalid.
    pub fn year_month(year: i32, month: u32) -> Result<Self, ProfileError> {
        Self::from_parts(year, month, 1, BirthDatePrecision::YearMonth)
    }

    /// Creates a birth date with full-date precision.
    ///
    /// Supported years are 1..=9999.
    ///
    /// # Errors
    ///
    /// Returns [ProfileError::InvalidInput] if the year is outside that range
    /// or the month or day is invalid.
    pub fn full_date(year: i32, month: u32, day: u32) -> Result<Self, ProfileError> {
        Self::from_parts(year, month, day, BirthDatePrecision::FullDate)
    }

    /// Parses a year, year-month, or full ISO date.
    ///
    /// Supported years are 1..=9999. Accepted forms are YYYY, YYYY-MM, and
    /// YYYY-MM-DD.
    ///
    /// # Errors
    ///
    /// Returns [ProfileError::InvalidInput] for an unsupported year, an
    /// invalid format, or invalid calendar components.
    pub fn parse(value: &str) -> Result<Self, ProfileError> {
        let invalid = || ProfileError::InvalidInput("invalid birth date".to_owned());
        match value.len() {
            4 if value.bytes().all(|byte| byte.is_ascii_digit()) => {
                let year = value.parse::<i32>().map_err(|_| invalid())?;
                Self::year(year)
            }
            7 if value.as_bytes().get(4) == Some(&b'-')
                && value
                    .get(..4)
                    .is_some_and(|year| year.bytes().all(|byte| byte.is_ascii_digit()))
                && value
                    .get(5..)
                    .is_some_and(|month| month.bytes().all(|byte| byte.is_ascii_digit())) =>
            {
                let year = value.get(..4).ok_or_else(invalid)?;
                let month = value.get(5..).ok_or_else(invalid)?;
                Self::year_month(
                    year.parse::<i32>().map_err(|_| invalid())?,
                    month.parse::<u32>().map_err(|_| invalid())?,
                )
            }
            10 if value.as_bytes().get(4) == Some(&b'-')
                && value.as_bytes().get(7) == Some(&b'-')
                && value
                    .get(..4)
                    .is_some_and(|year| year.bytes().all(|byte| byte.is_ascii_digit()))
                && value
                    .get(5..7)
                    .is_some_and(|month| month.bytes().all(|byte| byte.is_ascii_digit()))
                && value
                    .get(8..)
                    .is_some_and(|day| day.bytes().all(|byte| byte.is_ascii_digit())) =>
            {
                let year = value.get(..4).ok_or_else(invalid)?;
                let month = value.get(5..7).ok_or_else(invalid)?;
                let day = value.get(8..).ok_or_else(invalid)?;
                Self::full_date(
                    year.parse::<i32>().map_err(|_| invalid())?,
                    month.parse::<u32>().map_err(|_| invalid())?,
                    day.parse::<u32>().map_err(|_| invalid())?,
                )
            }
            _ => Err(invalid()),
        }
    }

    /// Returns the retained precision.
    pub const fn precision(self) -> BirthDatePrecision {
        self.precision
    }

    /// Returns the calendar date used for age calculations.
    pub const fn effective_date(self) -> NaiveDate {
        self.date
    }

    fn from_parts(
        year: i32,
        month: u32,
        day: u32,
        precision: BirthDatePrecision,
    ) -> Result<Self, ProfileError> {
        if !(1..=9999).contains(&year) {
            return Err(ProfileError::InvalidInput("invalid birth date".to_owned()));
        }
        let date = NaiveDate::from_ymd_opt(year, month, day)
            .ok_or_else(|| ProfileError::InvalidInput("invalid birth date".to_owned()))?;
        Ok(Self { date, precision })
    }
}

impl fmt::Display for BirthDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.precision {
            BirthDatePrecision::Year => write!(formatter, "{:04}", self.date.year()),
            BirthDatePrecision::YearMonth => {
                write!(
                    formatter,
                    "{:04}-{:02}",
                    self.date.year(),
                    self.date.month()
                )
            }
            BirthDatePrecision::FullDate => write!(
                formatter,
                "{:04}-{:02}-{:02}",
                self.date.year(),
                self.date.month(),
                self.date.day()
            ),
        }
    }
}

impl Serialize for BirthDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for BirthDate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A person's sex used by the selected BMR formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sex {
    /// Male formula constant.
    Male,
    /// Female formula constant.
    Female,
}

/// A validated IANA timezone identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IanaTimezone(Tz);

impl IanaTimezone {
    /// Parses an IANA timezone identifier.
    ///
    /// # Errors
    ///
    /// Returns [ProfileError::InvalidInput] if value is not a supported IANA
    /// timezone identifier.
    pub fn new(value: impl AsRef<str>) -> Result<Self, ProfileError> {
        Tz::from_str(value.as_ref())
            .map(Self)
            .map_err(|_| ProfileError::InvalidInput("invalid IANA timezone".to_owned()))
    }

    /// Returns the canonical timezone identifier.
    pub fn as_str(self) -> &'static str {
        self.0.name()
    }

    pub(crate) const fn timezone(self) -> Tz {
        self.0
    }
}

impl Default for IanaTimezone {
    fn default() -> Self {
        Self(Tz::UTC)
    }
}

impl Serialize for IanaTimezone {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for IanaTimezone {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A strictly positive body height in centimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyHeight(Centimetres);

impl BodyHeight {
    /// Creates a positive body height.
    ///
    /// # Errors
    ///
    /// Returns [ProfileError::InvalidInput] if value is zero.
    pub fn new(value: Centimetres) -> Result<Self, ProfileError> {
        if value.value().is_zero() {
            return Err(ProfileError::InvalidInput(
                "height must be greater than zero".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    /// Returns the height in centimetres.
    pub const fn value(self) -> Centimetres {
        self.0
    }
}

impl Serialize for BodyHeight {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BodyHeight {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Centimetres::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A strictly positive body weight in kilograms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyWeight(Kilograms);

impl BodyWeight {
    /// Creates a positive body weight.
    ///
    /// # Errors
    ///
    /// Returns [ProfileError::InvalidInput] if value is zero.
    pub fn new(value: Kilograms) -> Result<Self, ProfileError> {
        if value.value().is_zero() {
            return Err(ProfileError::InvalidInput(
                "weight must be greater than zero".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    /// Returns the weight in kilograms.
    pub const fn value(self) -> Kilograms {
        self.0
    }
}

impl Serialize for BodyWeight {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BodyWeight {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Kilograms::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A positive custom BMR value, stored as kcal/day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomBmr(Kilocalories);

impl CustomBmr {
    /// Creates a positive custom BMR value.
    ///
    /// # Errors
    ///
    /// Returns [ProfileError::InvalidInput] if value is zero.
    pub fn new(value: Kilocalories) -> Result<Self, ProfileError> {
        if value.value().is_zero() {
            return Err(ProfileError::InvalidInput(
                "custom BMR must be greater than zero".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    /// Returns the custom BMR value in kcal/day.
    pub const fn value(self) -> Kilocalories {
        self.0
    }
}

impl Serialize for CustomBmr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CustomBmr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Kilocalories::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Whether BMR is calculated from profile inputs or supplied explicitly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum BmrSetting {
    /// Calculate BMR from the profile's weight, height, age, and sex.
    #[default]
    Calculated,
    /// Use a supplied kcal/day value.
    Custom { value: CustomBmr },
}

impl<'de> Deserialize<'de> for BmrSetting {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match BmrSettingInput::deserialize(deserializer)? {
            BmrSettingInput::Calculated {} => Ok(Self::Calculated),
            BmrSettingInput::Custom { value } => Ok(Self::Custom { value }),
        }
    }
}

/// A preset used for baseline activity outside separately recorded activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityLevel {
    /// No baseline activity addition.
    NoActivity,
    /// A sedentary baseline.
    Sedentary,
    /// A lightly active baseline.
    LightlyActive,
    /// A moderately active baseline.
    ModeratelyActive,
    /// A very active baseline.
    VeryActive,
}

/// A preset activity level or a fixed custom baseline kcal/day value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum BaselineActivity {
    /// Calculate the addition from a preset level.
    Preset { level: ActivityLevel },
    /// Use a fixed nonnegative kcal/day addition.
    Custom { value: Kilocalories },
}

/// A profile's writable settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSettings {
    /// Optional birth date, with its original precision retained.
    pub birth_date: Option<BirthDate>,
    /// Sex used by the BMR formula.
    pub sex: Option<Sex>,
    /// Body height.
    pub height: Option<BodyHeight>,
    /// Local timezone used to evaluate the current date.
    #[serde(default)]
    pub timezone: IanaTimezone,
    /// Optional target body weight.
    pub weight_goal: Option<BodyWeight>,
    /// Selected BMR calculation mode.
    #[serde(default)]
    pub bmr: BmrSetting,
    /// Optional baseline activity addition.
    pub baseline_activity: Option<BaselineActivity>,
}

/// Read-only cached values derived from profile history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfileCache {
    /// The most recent recorded body weight, initially absent.
    pub current_weight: Option<BodyWeight>,
}

/// Settings and derived cache stored for a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileData {
    /// Writable profile settings.
    pub settings: ProfileSettings,
    /// Server-maintained values derived from profile history.
    pub cache: ProfileCache,
}

/// Calorie values calculated from settings and cache at read time.
///
/// Calculations use checked fixed-point decimal arithmetic. Precision
/// reduction uses midpoint rounding to nearest-even, and overflow is reported
/// as [ProfileError::Calculation].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDerived {
    /// Effective BMR, or None when required inputs are absent.
    pub bmr: Option<Kilocalories>,
    /// Baseline activity addition, or None when it cannot be calculated.
    pub baseline_activity: Option<Kilocalories>,
    /// Effective BMR plus baseline activity, or None if either is absent.
    pub maintenance_calories: Option<Kilocalories>,
}

/// A profile and its derived calorie values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Stable profile identifier.
    pub id: ProfileId,
    /// Display name.
    pub name: ProfileName,
    /// Profile settings and read-only cache.
    pub data: ProfileData,
    /// Values calculated from current settings and cache.
    pub derived: ProfileDerived,
}

/// Settings accepted when creating a profile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct CreateProfileSettings {
    /// Optional birth date.
    pub birth_date: Option<BirthDate>,
    /// Optional sex used by BMR calculation.
    pub sex: Option<Sex>,
    /// Optional body height.
    pub height: Option<BodyHeight>,
    /// Optional IANA timezone; defaults to UTC.
    pub timezone: Option<IanaTimezone>,
    /// Optional target body weight.
    pub weight_goal: Option<BodyWeight>,
    /// BMR mode; defaults to calculated.
    pub bmr: Option<BmrSetting>,
    /// Optional baseline activity mode.
    pub baseline_activity: Option<BaselineActivity>,
}

impl CreateProfileSettings {
    pub(crate) fn into_settings(self) -> ProfileSettings {
        ProfileSettings {
            birth_date: self.birth_date,
            sex: self.sex,
            height: self.height,
            timezone: self.timezone.unwrap_or_default(),
            weight_goal: self.weight_goal,
            bmr: self.bmr.unwrap_or_default(),
            baseline_activity: self.baseline_activity,
        }
    }
}

/// Input for creating a profile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateProfile {
    /// Profile display name.
    pub name: ProfileName,
    /// Optional profile settings.
    #[serde(default)]
    pub settings: Option<CreateProfileSettings>,
}

/// An unchanged or replacement value in a profile patch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Patch<T> {
    /// Keep the stored value.
    #[default]
    Unchanged,
    /// Replace the stored value.
    Set(T),
}

/// An unchanged, replacement, or clear operation for a nullable value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NullablePatch<T> {
    /// Keep the stored value.
    #[default]
    Unchanged,
    /// Replace the stored value.
    Set(T),
    /// Clear the stored value.
    Clear,
}

/// An unchanged BMR mode or an explicit reset to calculated mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BmrPatch {
    /// Keep the stored mode.
    Unchanged,
    /// Replace the mode.
    Set(BmrSetting),
    /// Reset to calculated mode.
    ResetToCalculated,
}

/// A partial update for a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchProfile {
    /// Optional replacement display name.
    pub name: Patch<ProfileName>,
    /// Optional birth date update.
    pub birth_date: NullablePatch<BirthDate>,
    /// Optional sex update.
    pub sex: NullablePatch<Sex>,
    /// Optional height update.
    pub height: NullablePatch<BodyHeight>,
    /// Optional timezone replacement.
    pub timezone: Patch<IanaTimezone>,
    /// Optional weight-goal update.
    pub weight_goal: NullablePatch<BodyWeight>,
    /// Optional BMR mode update; null resets to calculated mode.
    pub bmr: BmrPatch,
    /// Optional baseline activity update.
    pub baseline_activity: NullablePatch<BaselineActivity>,
}

impl<'de> Deserialize<'de> for PatchProfile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            #[serde(default, deserialize_with = "deserialize_present")]
            name: Option<Option<ProfileName>>,
            #[serde(default, deserialize_with = "deserialize_present")]
            birth_date: Option<Option<BirthDate>>,
            #[serde(default, deserialize_with = "deserialize_present")]
            sex: Option<Option<Sex>>,
            #[serde(default, deserialize_with = "deserialize_present")]
            height: Option<Option<BodyHeight>>,
            #[serde(default, deserialize_with = "deserialize_present")]
            timezone: Option<Option<IanaTimezone>>,
            #[serde(default, deserialize_with = "deserialize_present")]
            weight_goal: Option<Option<BodyWeight>>,
            #[serde(default, deserialize_with = "deserialize_present")]
            bmr: Option<Option<BmrSetting>>,
            #[serde(default, deserialize_with = "deserialize_present")]
            baseline_activity: Option<Option<BaselineActivity>>,
        }

        let fields = Fields::deserialize(deserializer)?;
        let name = match fields.name {
            None => Patch::Unchanged,
            Some(Some(name)) => Patch::Set(name),
            Some(None) => return Err(D::Error::custom("name cannot be null")),
        };
        let timezone = match fields.timezone {
            None => Patch::Unchanged,
            Some(Some(timezone)) => Patch::Set(timezone),
            Some(None) => return Err(D::Error::custom("timezone cannot be null")),
        };
        Ok(Self {
            name,
            birth_date: nullable_patch(fields.birth_date),
            sex: nullable_patch(fields.sex),
            height: nullable_patch(fields.height),
            timezone,
            weight_goal: nullable_patch(fields.weight_goal),
            bmr: match fields.bmr {
                None => BmrPatch::Unchanged,
                Some(Some(bmr)) => BmrPatch::Set(bmr),
                Some(None) => BmrPatch::ResetToCalculated,
            },
            baseline_activity: nullable_patch(fields.baseline_activity),
        })
    }
}

/// A bounded page size for profile listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ProfileLimit(i64);

impl ProfileLimit {
    /// Creates a page size in the range 1..=100.
    ///
    /// # Errors
    ///
    /// Returns [ProfileError::InvalidInput] if value is outside that range.
    pub fn new(value: i64) -> Result<Self, ProfileError> {
        if (1..=100).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ProfileError::InvalidInput(
                "profile limit must be between 1 and 100".to_owned(),
            ))
        }
    }

    /// Returns the bounded page size.
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ProfileLimit {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(i64::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Query options for an account's profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileListQuery {
    /// Exclusive ascending ID cursor.
    pub last_id: Option<ProfileId>,
    /// Maximum number of returned profiles; defaults to 100.
    pub limit: Option<ProfileLimit>,
}

impl ProfileListQuery {
    pub(crate) const fn effective_limit(self) -> i64 {
        match self.limit {
            Some(limit) => limit.get(),
            None => 100,
        }
    }
}

/// A page of profiles owned by one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileList {
    /// Profiles in ascending identifier order.
    pub items: Vec<Profile>,
    /// Exclusive cursor for the next page.
    pub last_id: Option<ProfileId>,
}

#[derive(Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum BmrSettingInput {
    Calculated {},
    Custom { value: CustomBmr },
}

fn deserialize_present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

fn nullable_patch<T>(value: Option<Option<T>>) -> NullablePatch<T> {
    match value {
        None => NullablePatch::Unchanged,
        Some(None) => NullablePatch::Clear,
        Some(Some(value)) => NullablePatch::Set(value),
    }
}

/// Builds public calculated values from profile settings and current weight.
pub(crate) fn derive_profile(
    settings: &ProfileSettings,
    cache: &ProfileCache,
    timestamp: UnixMillis,
) -> Result<ProfileDerived, ProfileError> {
    let current_day = local_day(settings.timezone, timestamp)?;
    let bmr = match settings.bmr {
        BmrSetting::Calculated => match (
            cache.current_weight,
            settings.height,
            settings.birth_date,
            settings.sex,
        ) {
            (Some(weight), Some(height), Some(birth_date), Some(sex)) => {
                let Some(age) = age_on(birth_date, current_day) else {
                    return Ok(ProfileDerived {
                        bmr: None,
                        baseline_activity: baseline_value(settings.baseline_activity, None)?,
                        maintenance_calories: None,
                    });
                };
                Some(calculate_bmr(weight, height, age, sex)?)
            }
            _ => None,
        },
        BmrSetting::Custom { value } => Some(value.value()),
    };
    let baseline_activity = baseline_value(settings.baseline_activity, bmr)?;
    let maintenance_calories = match (bmr, baseline_activity) {
        (Some(bmr), Some(addition)) => Some(add_kilocalories(bmr, addition)?),
        _ => None,
    };
    Ok(ProfileDerived {
        bmr,
        baseline_activity,
        maintenance_calories,
    })
}

fn local_day(timezone: IanaTimezone, timestamp: UnixMillis) -> Result<NaiveDate, ProfileError> {
    let utc = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp)
        .ok_or_else(|| ProfileError::Calculation("clock timestamp is out of range".to_owned()))?;
    Ok(utc.with_timezone(&timezone.timezone()).date_naive())
}

fn age_on(birth_date: BirthDate, current_day: NaiveDate) -> Option<i32> {
    let birthday = birth_date.effective_date();
    if birthday > current_day {
        return None;
    }
    let mut age = current_day.year().checked_sub(birthday.year())?;
    if (current_day.month(), current_day.day()) < (birthday.month(), birthday.day()) {
        age = age.checked_sub(1)?;
    }
    Some(age)
}

fn calculate_bmr(
    weight: BodyWeight,
    height: BodyHeight,
    age: i32,
    sex: Sex,
) -> Result<Kilocalories, ProfileError> {
    let mass = weight
        .value()
        .value()
        .checked_mul(Decimal::from(10))
        .ok_or_else(calculation_overflow)?;
    let height_component = height
        .value()
        .value()
        .checked_mul(Decimal::new(625, 2))
        .ok_or_else(calculation_overflow)?;
    let age_component = Decimal::from(age)
        .checked_mul(Decimal::from(5))
        .ok_or_else(calculation_overflow)?;
    let constant = match sex {
        Sex::Male => Decimal::from(5),
        Sex::Female => Decimal::from(-161),
    };
    let value = mass
        .checked_add(height_component)
        .and_then(|sum| sum.checked_sub(age_component))
        .and_then(|sum| sum.checked_add(constant))
        .ok_or_else(calculation_overflow)?;
    Kilocalories::try_new(value).map_err(|_| {
        ProfileError::Calculation("calculated BMR is outside the supported range".to_owned())
    })
}

fn baseline_value(
    setting: Option<BaselineActivity>,
    bmr: Option<Kilocalories>,
) -> Result<Option<Kilocalories>, ProfileError> {
    match setting {
        None => Ok(None),
        Some(BaselineActivity::Custom { value }) => Ok(Some(value)),
        Some(BaselineActivity::Preset { level }) => {
            let Some(bmr) = bmr else {
                return Ok(None);
            };
            let coefficient = match level {
                ActivityLevel::NoActivity => Decimal::ZERO,
                ActivityLevel::Sedentary => Decimal::new(2, 1),
                ActivityLevel::LightlyActive => Decimal::new(375, 3),
                ActivityLevel::ModeratelyActive => Decimal::new(5, 1),
                ActivityLevel::VeryActive => Decimal::new(9, 1),
            };
            let value = bmr
                .value()
                .checked_mul(coefficient)
                .ok_or_else(calculation_overflow)?;
            Ok(Some(Kilocalories::try_new(value).map_err(|_| {
                ProfileError::Calculation(
                    "baseline activity is outside the supported range".to_owned(),
                )
            })?))
        }
    }
}

fn add_kilocalories(left: Kilocalories, right: Kilocalories) -> Result<Kilocalories, ProfileError> {
    let value = left
        .value()
        .checked_add(right.value())
        .ok_or_else(calculation_overflow)?;
    Kilocalories::try_new(value).map_err(|_| {
        ProfileError::Calculation("maintenance calories are outside the supported range".to_owned())
    })
}

fn calculation_overflow() -> ProfileError {
    ProfileError::Calculation("calculated value is outside the supported range".to_owned())
}

impl From<QuantityError> for ProfileError {
    fn from(error: QuantityError) -> Self {
        Self::InvalidInput(error.to_string())
    }
}

/// Applies a patch while preserving cache and reporting whether writable state changed.
pub(crate) fn apply_patch(
    current_name: &ProfileName,
    current: &ProfileData,
    patch: &PatchProfile,
) -> (ProfileName, ProfileData, bool) {
    let mut name = current_name.clone();
    let mut settings = current.settings.clone();
    if let Patch::Set(replacement) = &patch.name {
        name = replacement.clone();
    }
    if let NullablePatch::Set(value) = &patch.birth_date {
        settings.birth_date = Some(*value);
    } else if patch.birth_date == NullablePatch::Clear {
        settings.birth_date = None;
    }
    if let NullablePatch::Set(value) = &patch.sex {
        settings.sex = Some(*value);
    } else if patch.sex == NullablePatch::Clear {
        settings.sex = None;
    }
    if let NullablePatch::Set(value) = &patch.height {
        settings.height = Some(*value);
    } else if patch.height == NullablePatch::Clear {
        settings.height = None;
    }
    if let Patch::Set(value) = patch.timezone {
        settings.timezone = value;
    }
    if let NullablePatch::Set(value) = &patch.weight_goal {
        settings.weight_goal = Some(*value);
    } else if patch.weight_goal == NullablePatch::Clear {
        settings.weight_goal = None;
    }
    match patch.bmr {
        BmrPatch::Unchanged => {}
        BmrPatch::Set(value) => settings.bmr = value,
        BmrPatch::ResetToCalculated => settings.bmr = BmrSetting::Calculated,
    }
    if let NullablePatch::Set(value) = patch.baseline_activity {
        settings.baseline_activity = Some(value);
    } else if patch.baseline_activity == NullablePatch::Clear {
        settings.baseline_activity = None;
    }
    let changed = name.as_str() != current_name.as_str() || settings != current.settings;
    (
        name,
        ProfileData {
            settings,
            cache: current.cache.clone(),
        },
        changed,
    )
}
