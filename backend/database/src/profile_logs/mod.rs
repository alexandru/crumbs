//! Per-profile measurement logs and their query contract.

mod error;
mod model;
mod sqlite;
mod store;

pub use error::ProfileLogError;
pub use model::{
    BiometricEntry, BiometricLogEntry, BiometricType, CreateLog, LocalTime, LogDay, LogEntryPatch,
    LogEntryType, LogFilter, LogInput, LogPage, PatchLog, ProfileLog, ProfileLogData, ProfileLogId,
    WeightMeasurement,
};
pub use store::ProfileLogs;

#[cfg(test)]
#[path = "../../unit-tests/profile_logs/mod.rs"]
mod tests;
