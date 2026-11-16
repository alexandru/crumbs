use std::path::PathBuf;

use serde::Deserialize;

pub type DatabasePath = PathBuf;

/// Configuration for [`Database`], to be loaded from the environment.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// Path to the (Sqlite) database file that we are using.
    ///
    /// TODO: This is a leaky abstraction, because we're trying to hide
    /// the fact that we are using Sqlite in the API, but we aren't
    /// doing a good job here. If we were using Postgres, we wouldn't
    /// have a DatabasePath, we'd have a connection string instead.
    /// To improve the API, we need to make it an enum, such that
    /// Sqlite is one of the possibilities.
    pub path: DatabasePath,
}

#[cfg(test)]
#[path = "../unit-tests/config.rs"]
mod tests;
