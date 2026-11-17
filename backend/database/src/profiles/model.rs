use chrono::{Datelike, NaiveDate};
use chrono_tz::Tz;
use regex::Regex;
use rust_decimal::{Decimal, dec};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::LazyLock;
use unicase::UniCase;
use unicode_normalization::UnicodeNormalization;

use crate::activity::{ActivityDuration, StepCount};
use crate::clock::UnixMillis;
use crate::nutrition_model::{
    Centimetres, Kcal, KcalPerCm, KcalPerKg, Kilograms, Measurement, NonNegative, PerDay, PerWeek,
    QuantityError, Volume,
};

use super::ProfileError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ProfileId(i64);

impl ProfileId {
    pub fn new(value: i64) -> Result<Self, ProfileError> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(ProfileError::InvalidInput("invalid profile ID".to_owned()))
        }
    }

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

#[derive(Debug, Clone)]
pub struct ProfileName {
    display: String,
    key: String,
}

impl ProfileName {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BirthDatePrecision {
    Year,
    YearMonth,
    FullDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BirthDate {
    date: NaiveDate,
    precision: BirthDatePrecision,
}

impl BirthDate {
    pub fn year(year: i32) -> Result<Self, ProfileError> {
        Self::from_parts(year, 1, 1, BirthDatePrecision::Year)
    }

    pub fn year_month(year: i32, month: u32) -> Result<Self, ProfileError> {
        Self::from_parts(year, month, 1, BirthDatePrecision::YearMonth)
    }

    pub fn full_date(year: i32, month: u32, day: u32) -> Result<Self, ProfileError> {
        Self::from_parts(year, month, day, BirthDatePrecision::FullDate)
    }

    pub fn parse(value: &str) -> Result<Self, ProfileError> {
        static FORMAT: LazyLock<Result<Regex, regex::Error>> =
            LazyLock::new(|| Regex::new(r"\A[0-9]{4}(?:-[0-9]{2}(?:-[0-9]{2})?)?\z"));

        let invalid = || ProfileError::InvalidInput("invalid birth date".to_owned());
        let format = FORMAT
            .as_ref()
            .map_err(|error| ProfileError::storage(error.clone()))?;
        if !format.is_match(value) {
            return Err(invalid());
        }

        let (date, precision) = match value.len() {
            4 => (
                Cow::Owned(format!("{value}-01-01")),
                BirthDatePrecision::Year,
            ),
            7 => (
                Cow::Owned(format!("{value}-01")),
                BirthDatePrecision::YearMonth,
            ),
            _ => (Cow::Borrowed(value), BirthDatePrecision::FullDate),
        };
        let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|_| invalid())?;
        if date.year() == 0 {
            return Err(invalid());
        }

        Ok(Self { date, precision })
    }

    pub const fn precision(self) -> BirthDatePrecision {
        self.precision
    }

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

    fn age_on(&self, current_day: NaiveDate) -> Option<AgeYears> {
        let birthday = self.effective_date();
        if birthday > current_day {
            return None;
        }
        let mut age = current_day.year() - birthday.year();
        if (current_day.month(), current_day.day()) < (birthday.month(), birthday.day()) {
            age -= 1;
        }
        Some(AgeYears(age))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sex {
    Male,
    Female,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IanaTimezone(Tz);

impl IanaTimezone {
    pub fn new(value: impl AsRef<str>) -> Result<Self, ProfileError> {
        Tz::from_str(value.as_ref())
            .map(Self)
            .map_err(|_| ProfileError::InvalidInput("invalid IANA timezone".to_owned()))
    }

    pub fn as_str(self) -> &'static str {
        self.0.name()
    }

    pub(crate) const fn timezone(self) -> Tz {
        self.0
    }

    fn local_day(&self, timestamp: UnixMillis) -> Result<NaiveDate, ProfileError> {
        let utc =
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp).ok_or_else(|| {
                ProfileError::Calculation("clock timestamp is out of range".to_owned())
            })?;
        Ok(utc.with_timezone(&self.0).date_naive())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct BodyHeight(Centimetres);

impl BodyHeight {
    pub fn new(value: Centimetres) -> Result<Self, ProfileError> {
        if !value.is_positive() {
            return Err(ProfileError::InvalidInput(
                "height must be greater than zero".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    pub const fn value(self) -> Centimetres {
        self.0
    }
}

impl<'de> Deserialize<'de> for BodyHeight {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Centimetres::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct BodyWeight(Kilograms);

impl BodyWeight {
    pub fn new(value: Kilograms) -> Result<Self, ProfileError> {
        if !value.is_positive() {
            return Err(ProfileError::InvalidInput(
                "weight must be greater than zero".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    pub const fn value(self) -> Kilograms {
        self.0
    }
}

impl<'de> Deserialize<'de> for BodyWeight {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Kilograms::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WeightChangeGoal(PerWeek<Kilograms>);

impl WeightChangeGoal {
    pub const fn new(value: PerWeek<Kilograms>) -> Self {
        Self(value)
    }

    pub const fn value(self) -> PerWeek<Kilograms> {
        self.0
    }

    pub fn calorie_balance_goal(self) -> PerDay<Kcal> {
        PerDay::new(KcalPerKg::from(7700) * self.0.amount() / dec!(7))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CustomBmr(PerDay<Kcal>);

impl CustomBmr {
    pub fn new(value: PerDay<Kcal>) -> Result<Self, ProfileError> {
        if !value.is_positive() {
            return Err(ProfileError::InvalidInput(
                "custom BMR must be greater than zero".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    pub const fn value(self) -> PerDay<Kcal> {
        self.0
    }
}

impl<'de> Deserialize<'de> for CustomBmr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(PerDay::<Kcal>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum BmrSetting {
    #[default]
    Calculated,
    Custom {
        value: CustomBmr,
    },
}

impl<'de> Deserialize<'de> for BmrSetting {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match BmrSettingInput::deserialize(deserializer)? {
            BmrSettingInput::Calculated {} => Ok(Self::Calculated),
            BmrSettingInput::Custom { value } => Ok(Self::Custom { value }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityLevel {
    NoActivity,
    Sedentary,
    LightlyActive,
    ModeratelyActive,
    VeryActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum BaselineActivity {
    Preset { level: ActivityLevel },
    Custom { value: PerDay<Kcal> },
}

impl BaselineActivity {
    fn baseline_value(&self, bmr: Option<PerDay<Kcal>>) -> Option<PerDay<Kcal>> {
        match *self {
            Self::Custom { value } => Some(value),
            Self::Preset { level } => {
                let bmr = bmr?;
                let coefficient = match level {
                    ActivityLevel::NoActivity => Decimal::ZERO,
                    ActivityLevel::Sedentary => dec!(0.2),
                    ActivityLevel::LightlyActive => dec!(0.375),
                    ActivityLevel::ModeratelyActive => dec!(0.5),
                    ActivityLevel::VeryActive => dec!(0.9),
                };
                Some(bmr * coefficient)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DailyWaterGoal(NonNegative<Volume>);

impl DailyWaterGoal {
    pub fn new(value: Volume) -> Result<Self, QuantityError> {
        NonNegative::new(value).map(Self)
    }

    pub fn value(self) -> Volume {
        *self.0.value()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSettings {
    pub birth_date: Option<BirthDate>,
    pub sex: Option<Sex>,
    pub height: Option<BodyHeight>,
    #[serde(default)]
    pub timezone: IanaTimezone,
    pub weight_goal: Option<BodyWeight>,
    pub weight_change_goal: Option<WeightChangeGoal>,
    pub daily_water_goal: Option<DailyWaterGoal>,
    pub daily_step_goal: Option<StepCount>,
    pub daily_active_time_goal: Option<ActivityDuration>,
    #[serde(default)]
    pub bmr: BmrSetting,
    pub baseline_activity: Option<BaselineActivity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfileCache {
    pub current_weight: Option<BodyWeight>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileData {
    pub settings: ProfileSettings,
    pub cache: ProfileCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDerived {
    bmr: Option<PerDay<Kcal>>,
    baseline_activity: Option<PerDay<Kcal>>,
    maintenance_calories: Option<PerDay<Kcal>>,
    calorie_balance_goal: Option<PerDay<Kcal>>,
}

impl ProfileDerived {
    pub fn from_components(
        bmr: Option<PerDay<Kcal>>,
        baseline_activity: Option<PerDay<Kcal>>,
    ) -> Self {
        let maintenance_calories = match (bmr, baseline_activity) {
            (Some(bmr), Some(activity)) => Some(bmr + activity),
            _ => None,
        };

        Self {
            bmr,
            baseline_activity,
            maintenance_calories,
            calorie_balance_goal: None,
        }
    }

    pub const fn bmr(&self) -> Option<PerDay<Kcal>> {
        self.bmr
    }
    pub const fn baseline_activity(&self) -> Option<PerDay<Kcal>> {
        self.baseline_activity
    }
    pub const fn maintenance_calories(&self) -> Option<PerDay<Kcal>> {
        self.maintenance_calories
    }

    pub const fn calorie_balance_goal(&self) -> Option<PerDay<Kcal>> {
        self.calorie_balance_goal
    }

    pub(crate) fn calculate(
        settings: &ProfileSettings,
        cache: &ProfileCache,
        timestamp: UnixMillis,
    ) -> Result<Self, ProfileError> {
        let current_day = settings.timezone.local_day(timestamp)?;
        let bmr = match settings.bmr {
            BmrSetting::Calculated => match (
                cache.current_weight,
                settings.height,
                settings.birth_date,
                settings.sex,
            ) {
                (Some(weight), Some(height), Some(birth_date), Some(sex)) => birth_date
                    .age_on(current_day)
                    .map(|age| calculate_bmr(weight, height, age, sex))
                    .transpose()?,
                _ => None,
            },
            BmrSetting::Custom { value } => Some(value.value()),
        };

        let baseline_activity = match settings.baseline_activity {
            Some(activity) => activity.baseline_value(bmr),
            None => None,
        };
        let mut derived = Self::from_components(bmr, baseline_activity);
        derived.calorie_balance_goal = settings
            .weight_change_goal
            .map(WeightChangeGoal::calorie_balance_goal);

        Ok(derived)
    }
}

impl<'de> Deserialize<'de> for ProfileDerived {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            bmr: Option<PerDay<Kcal>>,
            baseline_activity: Option<PerDay<Kcal>>,
            maintenance_calories: Option<PerDay<Kcal>>,
            calorie_balance_goal: Option<PerDay<Kcal>>,
        }
        let fields = Fields::deserialize(deserializer)?;

        let mut derived = Self::from_components(fields.bmr, fields.baseline_activity);
        if derived.maintenance_calories != fields.maintenance_calories {
            return Err(D::Error::custom(
                "maintenance calories do not match their components",
            ));
        }

        derived.calorie_balance_goal = fields.calorie_balance_goal;

        Ok(derived)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: ProfileId,
    pub name: ProfileName,
    pub data: ProfileData,
    pub derived: ProfileDerived,
}

impl Profile {
    pub(crate) fn from_data(
        id: ProfileId,
        name: ProfileName,
        data: ProfileData,
        now: UnixMillis,
    ) -> Result<Self, ProfileError> {
        let derived = ProfileDerived::calculate(&data.settings, &data.cache, now)?;

        Ok(Self {
            id,
            name,
            data,
            derived,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct CreateProfileSettings {
    pub birth_date: Option<BirthDate>,
    pub sex: Option<Sex>,
    pub height: Option<BodyHeight>,
    pub timezone: Option<IanaTimezone>,
    pub weight_goal: Option<BodyWeight>,
    pub weight_change_goal: Option<WeightChangeGoal>,
    pub daily_water_goal: Option<DailyWaterGoal>,
    pub daily_step_goal: Option<StepCount>,
    pub daily_active_time_goal: Option<ActivityDuration>,
    pub bmr: Option<BmrSetting>,
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
            weight_change_goal: self.weight_change_goal,
            daily_water_goal: self.daily_water_goal,
            daily_step_goal: self.daily_step_goal,
            daily_active_time_goal: self.daily_active_time_goal,
            bmr: self.bmr.unwrap_or_default(),
            baseline_activity: self.baseline_activity,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateProfile {
    pub name: ProfileName,
    #[serde(default)]
    pub settings: Option<CreateProfileSettings>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Patch<T> {
    #[default]
    Unchanged,
    Set(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Patch<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::Set)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NullablePatch<T> {
    #[default]
    Unchanged,
    Set(T),
    Clear,
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for NullablePatch<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Self::Set(value),
            None => Self::Clear,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchProfile {
    #[serde(default, deserialize_with = "deserialize_name_patch")]
    pub name: Patch<ProfileName>,
    #[serde(default)]
    pub birth_date: NullablePatch<BirthDate>,
    #[serde(default)]
    pub sex: NullablePatch<Sex>,
    #[serde(default)]
    pub height: NullablePatch<BodyHeight>,
    #[serde(default, deserialize_with = "deserialize_timezone_patch")]
    pub timezone: Patch<IanaTimezone>,
    #[serde(default)]
    pub weight_goal: NullablePatch<BodyWeight>,
    #[serde(default)]
    pub weight_change_goal: NullablePatch<WeightChangeGoal>,
    #[serde(default)]
    pub daily_water_goal: NullablePatch<DailyWaterGoal>,
    #[serde(default)]
    pub daily_step_goal: NullablePatch<StepCount>,
    #[serde(default)]
    pub daily_active_time_goal: NullablePatch<ActivityDuration>,
    #[serde(default)]
    pub bmr: NullablePatch<BmrSetting>,
    #[serde(default)]
    pub baseline_activity: NullablePatch<BaselineActivity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ProfileLimit(i64);

impl ProfileLimit {
    pub fn new(value: i64) -> Result<Self, ProfileError> {
        if (1..=100).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ProfileError::InvalidInput(
                "profile limit must be between 1 and 100".to_owned(),
            ))
        }
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ProfileLimit {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(i64::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileListQuery {
    pub last_id: Option<ProfileId>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileList {
    pub items: Vec<Profile>,
    pub last_id: Option<ProfileId>,
}

impl From<QuantityError> for ProfileError {
    fn from(error: QuantityError) -> Self {
        Self::InvalidInput(error.to_string())
    }
}

pub(crate) fn apply_patch(
    current_name: ProfileName,
    current: ProfileData,
    patch: &PatchProfile,
) -> (ProfileName, ProfileData, bool) {
    let (name, name_changed) = match &patch.name {
        Patch::Set(replacement) if replacement.as_str() != current_name.as_str() => {
            (replacement.clone(), true)
        }
        Patch::Unchanged | Patch::Set(_) => (current_name, false),
    };

    let mut settings = current.settings.clone();
    settings.birth_date = patched_optional(settings.birth_date, &patch.birth_date);
    settings.sex = patched_optional(settings.sex, &patch.sex);
    settings.height = patched_optional(settings.height, &patch.height);
    if let Patch::Set(value) = patch.timezone {
        settings.timezone = value;
    }
    settings.weight_goal = patched_optional(settings.weight_goal, &patch.weight_goal);
    settings.weight_change_goal =
        patched_optional(settings.weight_change_goal, &patch.weight_change_goal);
    settings.daily_water_goal =
        patched_optional(settings.daily_water_goal, &patch.daily_water_goal);
    settings.daily_step_goal = patched_optional(settings.daily_step_goal, &patch.daily_step_goal);
    settings.daily_active_time_goal = patched_optional(
        settings.daily_active_time_goal,
        &patch.daily_active_time_goal,
    );
    match patch.bmr {
        NullablePatch::Unchanged => {}
        NullablePatch::Set(value) => settings.bmr = value,
        NullablePatch::Clear => settings.bmr = BmrSetting::Calculated,
    }
    settings.baseline_activity =
        patched_optional(settings.baseline_activity, &patch.baseline_activity);

    let changed = name_changed || settings != current.settings;
    (
        name,
        ProfileData {
            settings,
            cache: current.cache,
        },
        changed,
    )
}

struct AgeYears(i32);

#[derive(Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum BmrSettingInput {
    Calculated {},
    Custom { value: CustomBmr },
}

fn deserialize_name_patch<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Patch<ProfileName>, D::Error> {
    Option::<ProfileName>::deserialize(deserializer)?
        .map(Patch::Set)
        .ok_or_else(|| D::Error::custom("name cannot be null"))
}

fn deserialize_timezone_patch<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Patch<IanaTimezone>, D::Error> {
    Option::<IanaTimezone>::deserialize(deserializer)?
        .map(Patch::Set)
        .ok_or_else(|| D::Error::custom("timezone cannot be null"))
}

fn calculate_bmr(
    weight: BodyWeight,
    height: BodyHeight,
    age: AgeYears,
    sex: Sex,
) -> Result<PerDay<Kcal>, ProfileError> {
    let mass = KcalPerKg::from(10) * weight.value();
    let height_component = KcalPerCm::from(dec!(6.25)) * height.value();
    let age_component = Kcal::from(5) * Decimal::from(age.0);
    let constant = match sex {
        Sex::Male => Kcal::from(5),
        Sex::Female => Kcal::from(-161),
    };
    let result = PerDay::<Kcal>::new(mass + height_component - age_component + constant);
    if result.is_negative() {
        return Err(ProfileError::Calculation(
            "calculated BMR is outside the supported range".to_owned(),
        ));
    }
    Ok(result)
}

fn patched_optional<T: Copy>(current: Option<T>, patch: &NullablePatch<T>) -> Option<T> {
    match patch {
        NullablePatch::Unchanged => current,
        NullablePatch::Set(value) => Some(*value),
        NullablePatch::Clear => None,
    }
}
