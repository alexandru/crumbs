//! Server scaffold for Crumbs.
//!
//! The process installs Unix shutdown handlers, announces readiness on standard
//! output, and waits for SIGHUP, SIGINT, or SIGTERM before exiting successfully.

#[cfg(not(unix))]
compile_error!("crumbs-server currently supports Unix targets only");

#[cfg(unix)]
use std::io::{self, Write};

#[cfg(unix)]
use crumbs_database::Database;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};

/// Arguments accepted by the `server` subcommand.
#[derive(Debug, clap::Args)]
pub struct ServerArgs {
    /// Operation to perform.
    #[command(subcommand)]
    pub command: ServerCommand,
}

/// Operations exposed by the `server` subcommand.
#[derive(Debug, clap::Subcommand)]
pub enum ServerCommand {
    /// Start the server and wait for a shutdown signal.
    Start,
}

/// Errors produced while starting or running the server.
#[cfg(unix)]
#[derive(Debug)]
pub enum Error {
    /// The tokio runtime could not be created.
    Runtime(io::Error),
    /// A Unix signal handler could not be installed.
    Signal(io::Error),
    /// The readiness message could not be written or flushed.
    Output(io::Error),
}

#[cfg(unix)]
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(source) => {
                write!(formatter, "failed to create the async runtime: {source}")
            }
            Self::Signal(source) => {
                write!(formatter, "failed to install a signal handler: {source}")
            }
            Self::Output(source) => {
                write!(formatter, "failed to write server output: {source}")
            }
        }
    }
}

#[cfg(unix)]
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Runtime(source) | Self::Signal(source) | Self::Output(source) => Some(source),
        }
    }
}

/// Runs the server until a shutdown signal arrives.
///
/// The database handle is owned for the lifetime of the process so request
/// handling can share it without reconnecting.
///
/// # Errors
///
/// Returns an error if the tokio runtime or Unix signal handlers cannot be
/// created, or if the readiness message cannot be written to standard output.
#[cfg(unix)]
pub fn start(database: Database) -> Result<(), Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)?;

    runtime.block_on(run_until_shutdown(database))
}

#[cfg(unix)]
async fn run_until_shutdown(database: Database) -> Result<(), Error> {
    let mut hangup = signal(SignalKind::hangup()).map_err(Error::Signal)?;
    let mut interrupt = signal(SignalKind::interrupt()).map_err(Error::Signal)?;
    let mut terminate = signal(SignalKind::terminate()).map_err(Error::Signal)?;

    {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "Hello, world!").map_err(Error::Output)?;
        stdout.flush().map_err(Error::Output)?;
    }

    // Kept until shutdown so the database handle lives as long as the server.
    let _database = database;

    tokio::select! {
        _ = hangup.recv() => Ok(()),
        _ = interrupt.recv() => Ok(()),
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{ServerArgs, ServerCommand};
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCommand,
    }

    #[derive(Debug, clap::Subcommand)]
    enum TestCommand {
        Server(ServerArgs),
    }

    #[test]
    fn parses_nested_server_start_command() -> Result<(), clap::Error> {
        let cli = TestCli::try_parse_from(["crumbs", "server", "start"])?;

        assert!(matches!(
            cli.command,
            TestCommand::Server(ServerArgs {
                command: ServerCommand::Start
            })
        ));
        Ok(())
    }

    #[test]
    fn rejects_unknown_server_operation() {
        assert!(TestCli::try_parse_from(["crumbs", "server", "stop"]).is_err());
    }
}
