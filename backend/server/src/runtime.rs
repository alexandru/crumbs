//! HTTP listener startup, readiness, and graceful shutdown.

use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use crumbs_database::FoodReader;
use tokio::net::TcpListener;
use tokio::signal::unix::{Signal, SignalKind, signal};

use crate::routes;
use crate::{Error, HttpConfig};

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

    let mcp_config = crate::mcp::config();
    let cancellation = mcp_config.cancellation_token.clone();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    let application = routes::application(reader, mcp_config);

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

async fn shutdown_signal(mut hangup: Signal, mut interrupt: Signal, mut terminate: Signal) {
    tokio::select! {
        _ = hangup.recv() => {}
        _ = interrupt.recv() => {}
        _ = terminate.recv() => {}
    }
}
