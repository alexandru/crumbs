use std::path::PathBuf;

use serde::Deserialize;

pub type DatabasePath = PathBuf;

/// Configuration for [`Database`], to be loaded from the environment.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// Path to the database file that we are using.
    pub path: DatabasePath,
}

#[cfg(test)]
#[path = "../unit-tests/config.rs"]
mod tests;
