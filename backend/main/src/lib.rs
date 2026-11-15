use std::error::Error as StdError;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use crumbs_database::{Database, DatabaseConfig};
use crumbs_import::{FoodCount, ImportDiagnostic, ImportError, ImportReport};
use crumbs_server::HttpConfig;

mod cli;
mod logging;
mod users;

pub use cli::{DatabaseArgs, DbArgs, DbCommand, HttpArgs, ServerArgs, ServerCommand};

#[derive(Debug, Parser)]
#[command(name = "crumbs", version, about = "Crumbs backend service")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Run the backend server")]
    Server(ServerArgs),
    #[command(about = "Manage the database")]
    Db(DbArgs),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to initialize logging: {0}")]
    Logging(#[source] Box<dyn StdError + Send + Sync>),
    #[error("failed to create the async runtime: {0}")]
    Runtime(#[source] io::Error),
    #[error(transparent)]
    Database(#[from] crumbs_database::Error),
    #[error(transparent)]
    Import(#[from] ImportError),
    #[error("rejected {} USDA foods", .count.get())]
    RejectedFoods { count: FoodCount },
    #[error(transparent)]
    CreateUser(#[from] users::Error),
    #[error("failed to write command output: {0}")]
    Output(#[source] io::Error),
    #[error(transparent)]
    Server(#[from] crumbs_server::Error),
}

pub fn run() -> Result<(), Error> {
    let cli = Cli::parse();
    logging::init().map_err(Error::Logging)?;

    run_cli(cli)
}

fn run_cli(cli: Cli) -> Result<(), Error> {
    match cli.command {
        Command::Server(ServerArgs { command }) => match command {
            ServerCommand::Start { database, http } => {
                runtime()?.block_on(run_server(database.into(), http.into()))
            }
        },
        Command::Db(DbArgs { command }) => match command {
            DbCommand::ImportUsda {
                file,
                database,
                verbose,
            } => runtime()?.block_on(run_import_usda(database.into(), file, verbose)),
            DbCommand::CreateUser { username, database } => {
                let (username, password_hash) = users::prompt_credentials(username, |prompt| {
                    rpassword::prompt_password(prompt)
                })?;
                let user = runtime()?.block_on(users::create_user(
                    database.into(),
                    username,
                    password_hash,
                ))?;
                let mut stdout = io::stdout().lock();
                writeln!(
                    stdout,
                    "Created user {} ({})",
                    user.username.as_str(),
                    user.id.get()
                )
                .map_err(Error::Output)?;
                stdout.flush().map_err(Error::Output)?;
                Ok(())
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

async fn run_server(database: DatabaseConfig, http: HttpConfig) -> Result<(), Error> {
    tracing::info!("server_starting");
    let database = Arc::new(Database::open(database).await?);
    tracing::info!("server_database_ready");

    let result = crumbs_server::start(Arc::clone(&database), http).await;
    database.close().await;
    if result.is_ok() {
        tracing::info!("server_stopped");
    } else {
        tracing::error!("server_failed");
    }

    result?;
    Ok(())
}

async fn run_import_usda(
    config: DatabaseConfig,
    file: PathBuf,
    verbose: bool,
) -> Result<(), Error> {
    tracing::info!(file = %file.display(), "import_command_started");
    let database = Database::open(config).await?;
    tracing::info!("import_database_ready");

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
