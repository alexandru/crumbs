//! Per-profile measurement logs and their query contract.

mod error;
mod exercise;
mod model;
mod sqlite;
mod store;

pub use error::ProfileLogError;
pub use exercise::{
    ExerciseCalories, ExerciseDuration, ExerciseIntensity, ExerciseLogEntry, ExerciseLogPatch,
    ExerciseNotes, ExerciseType,
};
pub use model::{
    Amount, BiometricEntry, BiometricLogEntry, BiometricType, CreateLog, FoodInputSnapshot,
    FoodLogEntry, FoodLogInput, FoodLogPatch, JournalLogEntry, JournalText, LocalTime, LogDay,
    LogEntryPatch, LogEntryType, LogFilter, LogInput, LogPage, MassUnit, MeasurementUnit, PatchLog,
    ProfileLog, ProfileLogData, ProfileLogId, SelectedServingName, ServingSelector,
    ServingSnapshot, VolumeUnit, WeightMeasurement,
};
pub use store::ProfileLogs;

#[cfg(test)]
#[path = "../../unit-tests/profile_logs/mod.rs"]
mod tests;
