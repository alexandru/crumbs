//! Application configuration loaded from a TOML file.

use std::path::Path;

// Leading `::` selects the external `config` crate rather than a local `config` module.
use ::config::{Config, ConfigError, File, FileFormat};
use crumbs_server::HttpConfig;
use serde::Deserialize;

/// Configuration for the `crumbs` executable.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// HTTP listener settings.
    pub http: HttpConfig,
}

impl AppConfig {
    /// Loads configuration from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read, the TOML is malformed, or
    /// the contents do not match [`AppConfig`].
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        Config::builder()
            .add_source(File::from(path.as_ref()).format(FileFormat::Toml))
            .build()?
            .try_deserialize()
    }
}

#[cfg(test)]
#[path = "../unit-tests/config.rs"]
mod tests;
