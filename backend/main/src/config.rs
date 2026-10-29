//! Application configuration loaded from a TOML file.

use std::path::Path;

// Leading `::` selects the external `config` crate rather than a local `config` module.
use ::config::{Config, ConfigError, File, FileFormat};
use crumbs_database::{DatabaseConfig, DatabasePath};
use crumbs_server::HttpConfig;
use serde::Deserialize;

/// Configuration for the `crumbs` executable.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Database settings.
    pub database: DatabaseConfig,
    /// HTTP listener settings.
    pub http: HttpConfig,
}

impl AppConfig {
    /// Loads configuration from a TOML file.
    ///
    /// A relative database path is resolved against the directory containing
    /// the configuration file. Absolute database paths are left unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read, the TOML is malformed, or
    /// the contents do not match [`AppConfig`].
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let mut config: Self = Config::builder()
            .add_source(File::from(path).format(FileFormat::Toml))
            .build()?
            .try_deserialize()?;
        config.database.path = resolve_database_path(path, config.database.path);
        Ok(config)
    }
}

fn resolve_database_path(config_path: &Path, database_path: DatabasePath) -> DatabasePath {
    if database_path.is_absolute() {
        return database_path;
    }

    match config_path.parent() {
        Some(parent) => parent.join(database_path),
        None => database_path,
    }
}

#[cfg(test)]
#[path = "../unit-tests/config.rs"]
mod tests;
