//! Per-profile measurement logs and their query contract.

mod error;
mod model;
mod sqlite;
mod store;

pub use error::ProfileLogError;
pub use model::{
    Amount, BiometricEntry, BiometricType, CreateLog, FoodInputSnapshot, FoodLogEntry,
    FoodLogInput, FoodLogPatch, LocalTime, LogDay, LogEntryPatch, LogEntryType, LogFilter,
    LogInput, LogPage, MassUnit, MeasurementUnit, PatchLog, ProfileLog, ProfileLogData,
    ProfileLogId, SelectedServingName, ServingSelector, ServingSnapshot, VolumeUnit,
    WeightMeasurement,
};
pub use store::ProfileLogs;

#[cfg(test)]
#[path = "../../unit-tests/profile_logs/mod.rs"]
mod tests;
