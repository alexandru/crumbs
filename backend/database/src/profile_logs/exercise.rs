use std::fmt;
use std::time::Duration;

use rust_decimal::Decimal;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::nutrition_model::{self, Kcal};
use crate::profiles::{NullablePatch, Patch};
use crate::text::NonBlankString;

use super::ProfileLogError;

/// The activity name as supplied, without a catalog reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExerciseType(pub NonBlankString);

/// The reported effort for an exercise session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExerciseIntensity {
    Easy,
    Moderate,
    Hard,
    VeryHard,
}

/// A positive exercise duration with nanosecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExerciseDuration(Duration);

impl ExerciseDuration {
    /// Creates a positive duration.
    ///
    /// # Errors
    ///
    /// Returns InvalidInput if the duration is zero.
    pub fn new(value: Duration) -> Result<Self, ProfileLogError> {
        if value.is_zero() {
            return Err(ProfileLogError::InvalidInput(
                "exercise duration must be positive".to_owned(),
            ));
        }

        Ok(Self(value))
    }

    /// Returns the duration.
    pub const fn value(self) -> Duration {
        self.0
    }

    fn parse(value: &str) -> Result<Self, ProfileLogError> {
        let invalid = || {
            ProfileLogError::InvalidInput(
                "exercise duration must be an exact decimal with unit s, min, or h".to_owned(),
            )
        };
        let (number, unit) = value
            .trim()
            .rsplit_once(char::is_whitespace)
            .ok_or_else(invalid)?;
        let factor: u128 = match unit {
            "s" => 1_000_000_000,
            "min" => 60_000_000_000,
            "h" => 3_600_000_000_000,
            _ => return Err(invalid()),
        };

        let decimal = nutrition_model::parse_decimal(number.trim()).map_err(|_| invalid())?;
        if decimal <= Decimal::ZERO {
            return Err(ProfileLogError::InvalidInput(
                "exercise duration must be positive".to_owned(),
            ));
        }

        let mantissa = u128::try_from(decimal.mantissa()).map_err(|_| invalid())?;
        let divisor = 10_u128.pow(decimal.scale());
        // Reduce the fraction before multiplication to test nanosecond precision without rounding.
        let common = gcd(divisor, factor);
        let divisor = divisor / common;
        let factor = factor / common;
        if mantissa % divisor != 0 {
            return Err(ProfileLogError::InvalidInput(
                "exercise duration must have nanosecond precision".to_owned(),
            ));
        }

        let count = mantissa / divisor;
        if count > Duration::MAX.as_nanos() / factor {
            return Err(ProfileLogError::InvalidInput(
                "exercise duration is too large".to_owned(),
            ));
        }

        let nanos = count * factor;
        let secs = u64::try_from(nanos / 1_000_000_000).map_err(|_| invalid())?;
        let subsec = u32::try_from(nanos % 1_000_000_000).map_err(|_| invalid())?;
        Self::new(Duration::new(secs, subsec))
    }
}

impl fmt::Display for ExerciseDuration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seconds =
            Decimal::from(self.0.as_secs()) + Decimal::new(i64::from(self.0.subsec_nanos()), 9);
        write!(formatter, "{} s", seconds.normalize())
    }
}

impl Serialize for ExerciseDuration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ExerciseDuration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Nonnegative energy burned during an exercise session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ExerciseCalories(Kcal);

impl ExerciseCalories {
    /// Creates an energy amount in kilocalories.
    ///
    /// # Errors
    ///
    /// Returns InvalidInput if the amount is negative.
    pub fn new(value: Kcal) -> Result<Self, ProfileLogError> {
        if value.is_negative() {
            return Err(ProfileLogError::InvalidInput(
                "calories burned must not be negative".to_owned(),
            ));
        }

        Ok(Self(value))
    }

    /// Returns the energy amount in kilocalories.
    pub const fn value(self) -> Kcal {
        self.0
    }
}

impl<'de> Deserialize<'de> for ExerciseCalories {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Kcal::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Free-text notes preserved as supplied.
pub type ExerciseNotes = String;

/// A self-contained exercise session with supplied energy expenditure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExerciseLogEntry {
    pub exercise_type: ExerciseType,
    pub intensity: ExerciseIntensity,
    pub duration: ExerciseDuration,
    pub calories_burned: ExerciseCalories,
    #[serde(default)]
    pub notes: Option<ExerciseNotes>,
}

/// Partial exercise changes. Null notes clear the stored notes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExerciseLogPatch {
    #[serde(default)]
    pub exercise_type: Patch<ExerciseType>,
    #[serde(default)]
    pub intensity: Patch<ExerciseIntensity>,
    #[serde(default)]
    pub duration: Patch<ExerciseDuration>,
    #[serde(default)]
    pub calories_burned: Patch<ExerciseCalories>,
    #[serde(default)]
    pub notes: NullablePatch<ExerciseNotes>,
}

impl ExerciseLogPatch {
    pub(super) fn apply(&self, current: &ExerciseLogEntry) -> ExerciseLogEntry {
        ExerciseLogEntry {
            exercise_type: replacement(&self.exercise_type, &current.exercise_type),
            intensity: replacement(&self.intensity, &current.intensity),
            duration: replacement(&self.duration, &current.duration),
            calories_burned: replacement(&self.calories_burned, &current.calories_burned),
            notes: match &self.notes {
                NullablePatch::Unchanged => current.notes.clone(),
                NullablePatch::Set(value) => Some(value.clone()),
                NullablePatch::Clear => None,
            },
        }
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn replacement<T: Clone>(patch: &Patch<T>, current: &T) -> T {
    match patch {
        Patch::Unchanged => current.clone(),
        Patch::Set(value) => value.clone(),
    }
}

#[cfg(test)]
#[path = "../../unit-tests/profile_logs/exercise.rs"]
mod tests;
