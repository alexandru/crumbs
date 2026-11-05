//! HTTP listener startup, readiness, and graceful shutdown.

use std::io::{self, Write};
use std::net::SocketAddr;
use std::sync::Arc;

use crumbs_database::FoodReader;
use crumbs_database::oauth::OAuthStore;
use crumbs_database::users::Users;
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
/// Returns an error if OAuth initialization fails, an insecure OAuth issuer is
/// configured with a non-loopback bind address, signal handlers cannot be
/// created, the listener cannot bind, its address cannot be read, the readiness
/// message cannot be written to standard output, or the HTTP server fails.
pub async fn start<S>(reader: Arc<S>, config: HttpConfig) -> Result<(), Error>
where
    S: FoodReader + Users + OAuthStore + Send + 'static,
{
    if !config.oauth.issuer.secure() && !config.bind.is_loopback() {
        return Err(Error::InsecureOAuthBind);
    }

    let authorization =
        crate::oauth::AuthorizationServer::new(Arc::clone(&reader), config.oauth).await?;
    // Handlers are installed before bind/readiness so Tokio buffers signals
    // that arrive during startup until the shutdown future is polled.
    let hangup = signal(SignalKind::hangup()).map_err(Error::Signal)?;
    let interrupt = signal(SignalKind::interrupt()).map_err(Error::Signal)?;
    let terminate = signal(SignalKind::terminate()).map_err(Error::Signal)?;

    let bind_address = SocketAddr::from((config.bind, config.port));
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
    let application = routes::application(Arc::clone(&reader), mcp_config, authorization);

    {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "Listening on http://{address}").map_err(Error::Output)?;
        stdout.flush().map_err(Error::Output)?;
    }

    let cleanup = async {
        let mut ticks = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            ticks.tick().await;
            if let Ok(now) = crate::oauth::protocol::now()
                && let Err(error) = reader.prune_oauth(now).await
            {
                eprintln!("OAuth cleanup failed: {error}");
            }
        }
    };
    let serving = axum::serve(
        listener,
        application.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal(hangup, interrupt, terminate).await;
        cancellation.cancel();
    })
    .into_future();
    tokio::pin!(serving, cleanup);
    tokio::select! {
        result = &mut serving => result.map_err(Error::Serve),
        _ = &mut cleanup => Ok(()),
    }
}

async fn shutdown_signal(mut hangup: Signal, mut interrupt: Signal, mut terminate: Signal) {
    tokio::select! {
        _ = hangup.recv() => {}
        _ = interrupt.recv() => {}
        _ = terminate.recv() => {}
    }
}
