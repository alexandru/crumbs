//! Command-line wiring for the `crumbs` executable.
//!
//! The clap hierarchy lives here so the executable entry point stays thin.
//! Server-owned argument types are composed into this hierarchy, keeping server
//! options close to their implementation.

use std::io;

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
    /// The async runtime could not be created.
    #[error("failed to create the async runtime: {0}")]
    Runtime(#[source] io::Error),
    /// The database could not be opened or migrated.
    #[error(transparent)]
    Database(#[from] crumbs_database::Error),
    /// The server failed to start or run.
    #[error(transparent)]
    Server(#[from] crumbs_server::Error),
}

/// Parses process arguments and runs the selected command.
///
/// # Errors
///
/// Returns an error if the configuration cannot be loaded, the runtime cannot
/// be created, the database cannot be opened or migrated, or server startup
/// fails.
pub fn run() -> Result<(), Error> {
    let cli = Cli::parse();
    run_cli(cli)
}

fn run_cli(cli: Cli) -> Result<(), Error> {
    match cli.command {
        Command::Server(ServerArgs { command }) => match command {
            ServerCommand::Start { config } => {
                let app_config = AppConfig::from_file(config)?;
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .map_err(Error::Runtime)?;
                runtime.block_on(run_server(app_config))
            }
        },
    }
}

async fn run_server(config: AppConfig) -> Result<(), Error> {
    let database = Database::open(config.database).await?;
    crumbs_server::start(&database, config.http).await?;
    Ok(())
}

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
