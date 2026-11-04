//! Command-line arguments for database operations.

use std::path::PathBuf;

use clap::{Args, Subcommand};

/// Arguments accepted by the `db` subcommand.
#[derive(Debug, Args)]
pub struct DbArgs {
    /// Operation to perform.
    #[command(subcommand)]
    pub command: DbCommand,
}

/// Operations exposed by the `db` subcommand.
#[derive(Debug, Subcommand)]
pub enum DbCommand {
    /// Import an uncompressed USDA FoodData Central JSON download.
    ImportUsda {
        /// Path to an uncompressed USDA FoodData Central JSON file (not ZIP).
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Path to the TOML configuration file.
        #[arg(long, value_name = "PATH")]
        config: PathBuf,
    },
}
