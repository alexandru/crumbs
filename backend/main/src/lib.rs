//! Command-line wiring for the `crumbs` executable.
//!
//! The clap hierarchy lives here so the executable entry point stays thin.
//! Server-owned argument types are composed into this hierarchy, keeping server
//! options close to their implementation.

use ::config::ConfigError;
use clap::{Parser, Subcommand};
use crumbs_database::Database;
use crumbs_server::{ServerArgs, ServerCommand};

mod config;

use config::AppConfig;

/// Top-level command-line interface for the `crumbs` executable.
#[derive(Debug, Parser)]
#[command(name = "crumbs", version, about = "Crumbs backend service")]
pub struct Cli {
    /// Operation to perform.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level operations exposed by the executable.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the backend server.
    Server(ServerArgs),
}

/// Errors produced while running the executable.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The application configuration could not be loaded.
    #[error("failed to load configuration: {0}")]
    Config(#[from] ConfigError),
    /// The server failed to start or run.
    #[error(transparent)]
    Server(#[from] crumbs_server::Error),
}

/// Parses process arguments and runs the selected command.
///
/// # Errors
///
/// Returns an error if the configuration cannot be loaded or server startup
/// fails.
pub fn run() -> Result<(), Error> {
    let cli = Cli::parse();
    run_cli(cli)
}

fn run_cli(cli: Cli) -> Result<(), Error> {
    match cli.command {
        Command::Server(ServerArgs { command }) => match command {
            ServerCommand::Start { config } => {
                let config = AppConfig::from_file(config)?;
                crumbs_server::start(Database, config.http)?;
                Ok(())
            }
        },
    }
}

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
