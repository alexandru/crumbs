//! Command-line wiring for the `crumbs` executable.
//!
//! The clap hierarchy lives here so the executable entry point stays thin.
//! Server argument types are composed with the application-owned database
//! commands in this hierarchy.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use ::config::ConfigError;
use clap::{Parser, Subcommand};
use crumbs_database::Database;
use crumbs_import::{FoodCount, ImportDiagnostic, ImportError, ImportReport};
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
    /// Some USDA food records were rejected.
    #[error("rejected {} USDA foods", .count.get())]
    RejectedFoods { count: FoodCount },
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
            DbCommand::ImportUsda {
                file,
                config,
                verbose,
            } => {
                let app_config = AppConfig::from_file(config)?;
                runtime()?.block_on(run_import_usda(app_config, file, verbose))
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
    let database = Arc::new(Database::open(config.database).await?);
    let result = crumbs_server::start(Arc::clone(&database), config.http).await;
    database.close().await;
    result?;
    Ok(())
}

async fn run_import_usda(config: AppConfig, file: PathBuf, verbose: bool) -> Result<(), Error> {
    let database = Database::open(config.database).await?;
    let import_result = crumbs_import::import_usda_with_diagnostics(
        &database,
        &file,
        |diagnostic: &ImportDiagnostic| {
            if verbose {
                let mut stderr = io::stderr().lock();
                writeln!(stderr, "{diagnostic}")?;
                stderr.flush()?;
            }
            Ok(())
        },
    )
    .await;
    database.close().await;
    let report = match import_result {
        Ok(report) => report,
        Err(error) => {
            write_import_summary(error.report())?;
            return Err(Error::Import(error));
        }
    };

    write_import_summary(report)?;
    if report.rejected().get() > 0 {
        return Err(Error::RejectedFoods {
            count: report.rejected(),
        });
    }
    Ok(())
}

fn write_import_summary(report: ImportReport) -> Result<(), Error> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Imported {} foods ({} sanitized); rejected {} foods",
        report.committed().get(),
        report.sanitized().get(),
        report.rejected().get()
    )
    .map_err(Error::Output)?;
    stdout.flush().map_err(Error::Output)
}

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
