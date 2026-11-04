//! HTTP route table for the server.

use std::sync::Arc;

use axum::Router;
use crumbs_database::FoodReader;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;

use crate::{handlers, mcp};

pub(crate) fn application<S>(reader: Arc<S>, mcp_config: StreamableHttpServerConfig) -> Router
where
    S: FoodReader + Send + 'static,
{
    Router::new()
        .route("/", axum::routing::get(handlers::hello))
        .route(
            "/foods/search",
            axum::routing::get(handlers::search_foods::<S>),
        )
        .with_state(Arc::clone(&reader))
        .nest_service("/mcp", mcp::service(reader, mcp_config))
}
