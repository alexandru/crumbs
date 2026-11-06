//! Typed configuration owned by the database crate.

use std::path::PathBuf;

use serde::Deserialize;

/// Location of the application database file.
pub type DatabasePath = PathBuf;

/// Settings for opening the application database.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// Path to the application database file.
    ///
    /// Relative paths use the process working directory.
    pub path: DatabasePath,
}

#[cfg(test)]
#[path = "../unit-tests/config.rs"]
mod tests;
