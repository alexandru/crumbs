use std::fmt;

use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::profiles::{BodyWeight, Patch, ProfileId};

use super::ProfileLogError;

/// A positive, stable identifier for one profile log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ProfileLogId(i64);

impl ProfileLogId {
    /// Creates a profile-log identifier from a positive integer.
    ///
    /// # Errors
    ///
    /// Returns InvalidInput if value is not positive.
    pub fn new(value: i64) -> Result<Self, ProfileLogError> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(ProfileLogError::InvalidInput(
                "profile-log ID must be positive".to_owned(),
            ))
        }
    }

    /// Returns the integer value of this identifier.
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ProfileLogId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(i64::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A validated ISO calendar day in the range year 1 through 9999.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogDay(NaiveDate);

impl LogDay {
    /// Creates a log day from calendar components.
    ///
    /// # Errors
    ///
    /// Returns InvalidInput for a year outside 1..=9999 or invalid month/day.
    pub fn new(year: i32, month: u32, day: u32) -> Result<Self, ProfileLogError> {
        if !(1..=9999).contains(&year) {
            return Err(invalid_day());
        }
        NaiveDate::from_ymd_opt(year, month, day)
            .map(Self)
            .ok_or_else(invalid_day)
    }

    /// Parses a day in YYYY-MM-DD form.
    ///
    /// # Errors
    ///
    /// Returns InvalidInput for another format or invalid calendar components.
    pub fn parse(value: &str) -> Result<Self, ProfileLogError> {
        if value.len() != 10
            || value.as_bytes().get(4) != Some(&b'-')
            || value.as_bytes().get(7) != Some(&b'-')
            || !value
                .get(..4)
                .is_some_and(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
            || !value
                .get(5..7)
                .is_some_and(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
            || !value
                .get(8..)
                .is_some_and(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(invalid_day());
        }
        let year = value.get(..4).and_then(|part| part.parse::<i32>().ok());
        let month = value.get(5..7).and_then(|part| part.parse::<u32>().ok());
        let day = value.get(8..).and_then(|part| part.parse::<u32>().ok());
        match (year, month, day) {
            (Some(year), Some(month), Some(day)) => Self::new(year, month, day),
            _ => Err(invalid_day()),
        }
    }

    /// Returns the year, month, and day components.
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

/// A validated local wall-clock time with millisecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalTime(NaiveTime);

impl LocalTime {
    /// Creates a local time from hour, minute, second, and millisecond parts.
    ///
    /// # Errors
    ///
    /// Returns InvalidInput for out-of-range time components.
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

    /// Parses HH:MM, HH:MM:SS, or HH:MM:SS with one to three fractional digits.
    ///
    /// # Errors
    ///
    /// Returns InvalidInput for an unsupported format, offset, leap second, or
    /// out-of-range component.
    pub fn parse(value: &str) -> Result<Self, ProfileLogError> {
        let (whole, fraction) = match value.split_once('.') {
            Some((whole, fraction)) => (whole, Some(fraction)),
            None => (value, None),
        };
        if let Some(fraction) = fraction
            && (whole.len() != 8
                || !(1..=3).contains(&fraction.len())
                || !fraction.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(invalid_time());
        }
        let (hour, minute, second) = match whole.len() {
            5 if whole.as_bytes().get(2) == Some(&b':') => (
                parse_two(whole.get(..2)),
                parse_two(whole.get(3..)),
                Some(0),
            ),
            8 if whole.as_bytes().get(2) == Some(&b':')
                && whole.as_bytes().get(5) == Some(&b':') =>
            {
                (
                    parse_two(whole.get(..2)),
                    parse_two(whole.get(3..5)),
                    parse_two(whole.get(6..)),
                )
            }
            _ => return Err(invalid_time()),
        };
        let millisecond = match fraction {
            None => 0,
            Some(fraction) => {
                let parsed = fraction.parse::<u32>().map_err(|_| invalid_time())?;
                match fraction.len() {
                    1 => parsed * 100,
                    2 => parsed * 10,
                    3 => parsed,
                    _ => return Err(invalid_time()),
                }
            }
        };
        match (hour, minute, second) {
            (Some(hour), Some(minute), Some(second)) => {
                Self::new(hour, minute, second, millisecond)
            }
            _ => Err(invalid_time()),
        }
    }

    /// Returns hour, minute, second, and millisecond components.
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

/// A measured body weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightMeasurement {
    /// Positive weight in kilograms.
    pub value: BodyWeight,
}

/// A supported physical measurement subtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BiometricType {
    /// Body weight in kilograms.
    Weight,
}

impl BiometricType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Weight => "weight",
        }
    }
}

/// A biometric entry supported by this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BiometricEntry {
    /// A body-weight measurement.
    Weight(WeightMeasurement),
}

/// The standalone JSON payload for a profile log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileLogData {
    /// A biometric entry.
    Biometric { measurement: BiometricEntry },
}

/// A log entry accepted on create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LogInput {
    /// A biometric measurement.
    Biometric { measurement: BiometricEntry },
}

impl LogInput {
    pub(crate) const fn into_data(self) -> ProfileLogData {
        match self {
            Self::Biometric { measurement } => ProfileLogData::Biometric { measurement },
        }
    }
}

/// Input for creating a profile log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateLog {
    /// Required local calendar day.
    pub day: LogDay,
    /// Optional local time. Omission resolves to the current profile-local time.
    pub time: Option<LocalTime>,
    /// Weight entry data.
    pub entry: LogInput,
}

/// An entry variant accepted for a log patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LogEntryPatch {
    /// Replace the biometric measurement.
    Biometric { measurement: BiometricEntry },
}

impl LogEntryPatch {
    pub(crate) const fn into_data(self) -> ProfileLogData {
        match self {
            Self::Biometric { measurement } => ProfileLogData::Biometric { measurement },
        }
    }
}

/// A partial patch for a profile log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchLog {
    /// Replacement local day.
    #[serde(default)]
    pub day: Patch<LogDay>,
    /// Replacement local time.
    #[serde(default)]
    pub time: Patch<LocalTime>,
    /// Replacement entry of the existing variant.
    #[serde(default)]
    pub entry: Patch<LogEntryPatch>,
}

/// A queryable top-level log category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogEntryType {
    /// Food consumption, supported in a later slice.
    Food,
    /// Biometric measurements.
    Biometric,
}

impl LogEntryType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Food => "food",
            Self::Biometric => "biometric",
        }
    }
}

/// Validated filters for a profile's log page.
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
    /// Creates a validated log filter.
    ///
    /// # Errors
    ///
    /// Returns InvalidInput if from is later than to or subtype is supplied
    /// without the biometric entry type.
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

    /// Returns the inclusive lower day bound.
    pub const fn from(&self) -> Option<LogDay> {
        self.from
    }

    /// Returns the inclusive upper day bound.
    pub const fn to(&self) -> Option<LogDay> {
        self.to
    }

    /// Returns the entry-type filter.
    pub const fn entry_type(&self) -> Option<LogEntryType> {
        self.entry_type
    }

    /// Returns the biometric subtype filter.
    pub const fn subtype(&self) -> Option<BiometricType> {
        self.subtype
    }

    /// Returns the exclusive descending ID cursor.
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

/// A decoded profile-log row without persistence timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileLog {
    /// Stable log identifier.
    pub id: ProfileLogId,
    /// Owning profile identifier.
    pub profile_id: ProfileId,
    /// Person-local calendar day.
    pub day: LogDay,
    /// Resolved person-local clock time.
    pub time: LocalTime,
    /// Standalone entry payload.
    pub data: ProfileLogData,
}

/// A fixed page of logs ordered by descending ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogPage {
    /// Log entries in descending ID order.
    pub items: Vec<ProfileLog>,
    /// Last returned ID, or None for an empty page.
    pub last_id: Option<ProfileLogId>,
}

fn parse_two(value: Option<&str>) -> Option<u32> {
    let value = value?;
    if value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        value.parse().ok()
    } else {
        None
    }
}

fn invalid_day() -> ProfileLogError {
    ProfileLogError::InvalidInput("day must be a valid YYYY-MM-DD date".to_owned())
}

fn invalid_time() -> ProfileLogError {
    ProfileLogError::InvalidInput("time must be a valid local HH:MM[:SS[.mmm]] value".to_owned())
}
