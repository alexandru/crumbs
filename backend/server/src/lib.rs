//! HTTP server for Crumbs.

#[cfg(not(unix))]
compile_error!("crumbs-server currently supports Unix targets only");

use std::io;

mod config;
mod custom_foods;
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
