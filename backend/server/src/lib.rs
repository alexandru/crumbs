//! HTTP server for Crumbs.

#[cfg(not(unix))]
compile_error!("crumbs-server currently supports Unix targets only");

use std::io;
use std::path::PathBuf;

mod config;
mod handlers;
mod mcp;
mod oauth;
mod oauth_views;
mod routes;
mod runtime;
mod search;

pub use config::{HttpBindAddress, HttpConfig, HttpPort, InvalidIssuerUrl, IssuerUrl, OAuthConfig};
pub use oauth::InitError as OAuthInitError;
pub use runtime::start;

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
    Start {
        /// Path to the TOML configuration file.
        #[arg(long, value_name = "PATH")]
        config: PathBuf,
    },
}

/// Errors produced while starting or running the server.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// OAuth services could not be initialized.
    #[error("failed to initialize OAuth: {0}")]
    OAuth(#[from] oauth::InitError),
    /// A Unix signal handler could not be installed.
    #[error("failed to install a signal handler: {0}")]
    Signal(#[source] io::Error),
    /// The HTTP listener could not bind to the configured port.
    #[error("failed to bind the HTTP listener to port {port}: {source}")]
    Bind {
        port: HttpPort,
        #[source]
        source: io::Error,
    },
    /// An HTTP OAuth issuer attempted to bind outside loopback.
    #[error("HTTP OAuth issuer requires a loopback bind address")]
    InsecureOAuthBind,
    /// The bound listener did not report its local address.
    #[error("failed to read the bound listener address: {0}")]
    LocalAddr(#[source] io::Error),
    /// The readiness message could not be written or flushed.
    #[error("failed to write server output: {0}")]
    Output(#[source] io::Error),
    /// The HTTP server stopped with an error.
    #[error("failed to serve HTTP requests: {0}")]
    Serve(#[source] io::Error),
}

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
