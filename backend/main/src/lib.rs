//! Command-line wiring for the `crumbs` executable.
//!
//! The clap hierarchy lives here so the executable entry point stays thin.
//! Server argument types are composed with the application-owned database
//! commands in this hierarchy.

use std::io::{self, Write};
use std::path::PathBuf;

use ::config::ConfigError;
use clap::{Parser, Subcommand};
use crumbs_database::Database;
use crumbs_import::ImportError;
use crumbs_server::{ServerArgs, ServerCommand};

mod cli;
mod config;

pub use cli::{DbArgs, DbCommand};
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
    /// Manage the database.
    Db(DbArgs),
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
    /// A USDA import failed.
    #[error(transparent)]
    Import(#[from] ImportError),
    /// Command output could not be written.
    #[error("failed to write command output: {0}")]
    Output(#[source] io::Error),
    /// The server failed to start or run.
    #[error(transparent)]
    Server(#[from] crumbs_server::Error),
}

/// Parses process arguments and runs the selected command.
///
/// # Errors
///
/// Returns an error if the configuration cannot be loaded, the runtime cannot
/// be created, the database cannot be opened or migrated, an import fails, or
/// server startup fails.
pub fn run() -> Result<(), Error> {
    let cli = Cli::parse();
    run_cli(cli)
}

fn run_cli(cli: Cli) -> Result<(), Error> {
    match cli.command {
        Command::Server(ServerArgs { command }) => match command {
            ServerCommand::Start { config } => {
                let app_config = AppConfig::from_file(config)?;
                runtime()?.block_on(run_server(app_config))
            }
        },
        Command::Db(DbArgs { command }) => match command {
            DbCommand::ImportUsda { file, config } => {
                let app_config = AppConfig::from_file(config)?;
                runtime()?.block_on(run_import_usda(app_config, file))
            }
        },
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, Error> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(Error::Runtime)
}

async fn run_server(config: AppConfig) -> Result<(), Error> {
    let database = Database::open(config.database).await?;
    let result = crumbs_server::start(&database, config.http).await;
    database.close().await;
    result?;
    Ok(())
}

async fn run_import_usda(config: AppConfig, file: PathBuf) -> Result<(), Error> {
    let database = Database::open(config.database).await?;
    let import_result = crumbs_import::import_usda(&database, &file).await;
    database.close().await;
    let report = import_result?;

    let mut stdout = io::stdout().lock();
    writeln!(stdout, "Imported {} foods", report.committed().get()).map_err(Error::Output)?;
    stdout.flush().map_err(Error::Output)?;
    Ok(())
}

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
