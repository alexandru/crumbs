//! Command-line wiring for the `crumbs` executable.
//!
//! The clap hierarchy lives here so the executable entry point stays thin.
//! Server-owned argument types are composed into this hierarchy, keeping server
//! options close to their implementation.

use clap::{Parser, Subcommand};
use crumbs_database::Database;
use crumbs_server::{ServerArgs, ServerCommand};

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

/// Parses process arguments and runs the selected command.
///
/// # Errors
///
/// Returns an error if server startup fails.
pub fn run() -> Result<(), crumbs_server::Error> {
    let cli = Cli::parse();
    run_cli(cli)
}

fn run_cli(cli: Cli) -> Result<(), crumbs_server::Error> {
    match cli.command {
        Command::Server(ServerArgs { command }) => match command {
            ServerCommand::Start => crumbs_server::start(Database),
        },
    }
}

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
