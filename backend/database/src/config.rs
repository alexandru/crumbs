//! Typed configuration owned by the database crate.

use std::path::PathBuf;

use serde::Deserialize;

/// Location of the SQLite database file.
pub type DatabasePath = PathBuf;

/// Settings for opening the application database.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// Path to the SQLite database file.
    ///
    /// Relative paths are resolved by the executable that loads the application
    /// configuration; the database crate uses the path it is given.
    pub path: DatabasePath,
}

#[cfg(test)]
#[path = "../unit-tests/config.rs"]
mod tests;
