use std::path::PathBuf;

use serde::Deserialize;

pub type DatabasePath = PathBuf;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    pub path: DatabasePath,
}

#[cfg(test)]
#[path = "../unit-tests/config.rs"]
mod tests;
