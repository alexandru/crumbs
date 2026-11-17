use crumbs_derive::{AliasTryNew, AliasValue};
use serde::{Deserialize, Serialize};

use crate::nutrition_model::{Kcal, NonNegative};
use crate::profiles::{NullablePatch, Patch};
use crate::text::NonBlankString;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, AliasTryNew, AliasValue)]
#[serde(transparent)]
pub struct ExerciseType(NonBlankString);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExerciseIntensity {
    Easy,
    Moderate,
    Hard,
    VeryHard,
}

pub type ExerciseDuration = crate::activity::ActivityDuration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AliasTryNew, AliasValue)]
#[serde(transparent)]
pub struct ExerciseCalories(NonNegative<Kcal>);

pub type ExerciseNotes = String;

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
