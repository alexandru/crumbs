use std::io::{self, Write};
use std::net::SocketAddr;
use std::sync::Arc;

use crumbs_database::daily_status::DailyStatusStore;
use crumbs_database::foods::{CustomFoodStore, FoodSearch};
use crumbs_database::identity::{OAuthStore, Users};
use crumbs_database::profile_logs::ProfileLogs;
use crumbs_database::profiles::Profiles;
use tokio::net::TcpListener;
use tokio::signal::unix::{Signal, SignalKind, signal};

use crate::config::{HttpConfig, HttpPort};
use crate::{oauth, routes};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to initialize OAuth: {0}")]
    OAuth(#[from] oauth::InitError),
    #[error("failed to install a signal handler: {0}")]
    Signal(#[source] io::Error),
    #[error("failed to bind the HTTP listener to port {port}: {source}")]
    Bind {
        port: HttpPort,
        #[source]
        source: io::Error,
    },
    #[error("HTTP OAuth issuer requires a loopback bind address")]
    InsecureOAuthBind,
    #[error("failed to read the bound listener address: {0}")]
    LocalAddr(#[source] io::Error),
    #[error("failed to write server output: {0}")]
    Output(#[source] io::Error),
    #[error("failed to serve HTTP requests: {0}")]
    Serve(#[source] io::Error),
}

pub async fn start<S>(reader: Arc<S>, config: HttpConfig) -> Result<(), Error>
where
    S: FoodSearch
        + CustomFoodStore
        + Profiles
        + ProfileLogs
        + DailyStatusStore
        + Users
        + OAuthStore
        + Send
        + Sync
        + 'static,
{
    if !config.oauth.issuer.secure() && !config.bind.is_loopback() {
        return Err(Error::InsecureOAuthBind);
    }

    let authorization =
        crate::oauth::AuthorizationServer::new(Arc::clone(&reader), config.oauth).await?;
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
    let application =
        routes::application(Arc::clone(&reader), mcp_config, Arc::clone(&authorization));

    {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "Listening on http://{address}").map_err(Error::Output)?;
        stdout.flush().map_err(Error::Output)?;
    }

    let cleanup = authorization.cleanup();
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
