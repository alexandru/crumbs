use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize};

use crate::nutrition_model::{Kcal, Minutes};
use crate::profiles::{NullablePatch, Patch};

use super::ProfileLogError;

/// The activity name as supplied, without a catalog reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ExerciseType(String);

impl ExerciseType {
    /// Creates an activity name and preserves its text.
    ///
    /// # Errors
    ///
    /// Returns InvalidInput if the name is blank.
    pub fn new(value: impl Into<String>) -> Result<Self, ProfileLogError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ProfileLogError::InvalidInput(
                "exercise type must not be blank".to_owned(),
            ));
        }

        Ok(Self(value))
    }

    /// Returns the activity name as supplied.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ExerciseType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// The reported effort for an exercise session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExerciseIntensity {
    Easy,
    Moderate,
    Hard,
    VeryHard,
}

/// A positive, exact exercise duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ExerciseDuration(Minutes);

impl ExerciseDuration {
    /// Creates a positive duration.
    ///
    /// # Errors
    ///
    /// Returns InvalidInput if the duration is zero or negative.
    pub fn new(value: Minutes) -> Result<Self, ProfileLogError> {
        if !value.is_positive() {
            return Err(ProfileLogError::InvalidInput(
                "exercise duration must be positive".to_owned(),
            ));
        }

        Ok(Self(value))
    }

    /// Returns the exact duration in minutes.
    pub const fn value(self) -> Minutes {
        self.0
    }
}

impl<'de> Deserialize<'de> for ExerciseDuration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Minutes::deserialize(deserializer)?).map_err(D::Error::custom)
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

fn replacement<T: Clone>(patch: &Patch<T>, current: &T) -> T {
    match patch {
        Patch::Unchanged => current.clone(),
        Patch::Set(value) => value.clone(),
    }
}

#[cfg(test)]
#[path = "../../unit-tests/profile_logs/exercise.rs"]
mod tests;
