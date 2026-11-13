use std::fmt;
use std::time::Duration;

use crumbs_derive::{AliasNew, AliasTryNew, AliasValue};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::nutrition_model::{Kcal, NonNegative};
use crate::profiles::{NullablePatch, Patch};
use crate::text::NonBlankString;

/// The activity name as supplied, without a catalog reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, AliasTryNew, AliasValue)]
#[serde(transparent)]
pub struct ExerciseType(NonBlankString);

/// The reported effort for an exercise session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExerciseIntensity {
    Easy,
    Moderate,
    Hard,
    VeryHard,
}

/// An exercise duration with nanosecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, AliasNew, AliasValue)]
pub struct ExerciseDuration(Duration);

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
        humantime::parse_duration(&String::deserialize(deserializer)?)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

/// Nonnegative energy burned during an exercise session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AliasTryNew, AliasValue)]
#[serde(transparent)]
pub struct ExerciseCalories(NonNegative<Kcal>);

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

fn replacement<T: Clone>(patch: &Patch<T>, current: &T) -> T {
    match patch {
        Patch::Unchanged => current.clone(),
        Patch::Set(value) => value.clone(),
    }
}

#[cfg(test)]
#[path = "../../unit-tests/profile_logs/exercise.rs"]
mod tests;
