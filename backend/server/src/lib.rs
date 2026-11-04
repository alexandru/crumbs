//! HTTP server for Crumbs.
//!
//! The server receives typed HTTP settings, binds the listener, announces
//! readiness on standard output, and serves until SIGHUP, SIGINT, or SIGTERM
//! arrives.

#[cfg(not(unix))]
compile_error!("crumbs-server currently supports Unix targets only");

use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::{Json, Router};
use crumbs_database::FoodReader;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::signal::unix::{Signal, SignalKind, signal};

mod config;
mod mcp;
mod search;

pub use config::{HttpConfig, HttpPort};

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

/// Runs the HTTP server until a shutdown signal arrives.
///
/// Port `0` in `config` lets the operating system choose an available port; the
/// readiness message reports the actual bound address.
///
/// # Errors
///
/// Returns an error if signal handlers cannot be created, the listener cannot
/// bind, its address cannot be read, the readiness message cannot be written to
/// standard output, or the HTTP server fails.
pub async fn start<S>(reader: Arc<S>, config: HttpConfig) -> Result<(), Error>
where
    S: FoodReader + Send + 'static,
{
    // Handlers are installed before bind/readiness so Tokio buffers signals
    // that arrive during startup until the shutdown future is polled.
    let hangup = signal(SignalKind::hangup()).map_err(Error::Signal)?;
    let interrupt = signal(SignalKind::interrupt()).map_err(Error::Signal)?;
    let terminate = signal(SignalKind::terminate()).map_err(Error::Signal)?;

    let bind_address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, config.port));
    let listener = TcpListener::bind(bind_address)
        .await
        .map_err(|source| Error::Bind {
            port: config.port,
            source,
        })?;
    let address = listener.local_addr().map_err(Error::LocalAddr)?;

    let mcp_config = mcp::config();
    let cancellation = mcp_config.cancellation_token.clone();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    let application = application(reader, mcp_config);

    {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "Listening on http://{address}").map_err(Error::Output)?;
        stdout.flush().map_err(Error::Output)?;
    }

    axum::serve(listener, application)
        .with_graceful_shutdown(async move {
            shutdown_signal(hangup, interrupt, terminate).await;
            cancellation.cancel();
        })
        .await
        .map_err(Error::Serve)
}

fn application<S>(reader: Arc<S>, mcp_config: StreamableHttpServerConfig) -> Router
where
    S: FoodReader + Send + 'static,
{
    Router::new()
        .route("/", axum::routing::get(hello))
        .route("/foods/search", axum::routing::get(search::handler::<S>))
        .with_state(Arc::clone(&reader))
        .merge(mcp::router(reader, mcp_config))
}

async fn shutdown_signal(mut hangup: Signal, mut interrupt: Signal, mut terminate: Signal) {
    tokio::select! {
        _ = hangup.recv() => {}
        _ = interrupt.recv() => {}
        _ = terminate.recv() => {}
    }
}

#[derive(Debug, Serialize)]
struct Greeting {
    greeting: &'static str,
}

async fn hello() -> Json<Greeting> {
    Json(Greeting {
        greeting: "Hello, world!",
    })
}

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
