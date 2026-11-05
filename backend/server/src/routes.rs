//! HTTP route table for the server.

use std::sync::Arc;

use axum::Router;
use crumbs_database::FoodReader;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;

use crumbs_database::oauth::OAuthStore;
use crumbs_database::users::Users;

use crate::{handlers, mcp, oauth};

pub(crate) fn application<S, A>(
    reader: Arc<S>,
    mcp_config: StreamableHttpServerConfig,
    authorization: Arc<oauth::AuthorizationServer<A>>,
) -> Router
where
    S: FoodReader + Send + 'static,
    A: Users + OAuthStore + Send + 'static,
{
    Router::new()
        .route(
            "/foods/search",
            axum::routing::get(handlers::search_foods::<S>),
        )
        .with_state(Arc::clone(&reader))
        .nest_service("/mcp", mcp::service(reader, mcp_config))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&authorization),
            oauth::require_access::<A>,
        ))
        .route("/", axum::routing::get(handlers::hello))
        .merge(oauth::routes(authorization))
        .layer(axum::middleware::from_fn(oauth::security_headers))
}
