//! HTTP route table for the server.

use std::sync::Arc;

use axum::Router;
use crumbs_database::foods::{CustomFoodStore, FoodSearch};
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;

use crumbs_database::identity::{OAuthStore, Users};

use crate::{assets, custom_foods, home, mcp, oauth, search};

pub(crate) fn application<S, A>(
    reader: Arc<S>,
    mcp_config: StreamableHttpServerConfig,
    authorization: Arc<oauth::AuthorizationServer<A>>,
) -> Router
where
    S: FoodSearch + CustomFoodStore + Send + 'static,
    A: Users + OAuthStore + Send + 'static,
{
    let tools = search::mcp_tools::<S>().chain(custom_foods::mcp_tools::<S>());
    search::routes(Arc::clone(&reader))
        .merge(custom_foods::routes(Arc::clone(&reader)))
        .nest_service(
            "/mcp",
            mcp::service(reader, mcp_config, tools, MCP_INSTRUCTIONS),
        )
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&authorization),
            oauth::require_access::<A>,
        ))
        .merge(home::routes())
        .merge(oauth::routes(authorization))
        .merge(assets::routes())
        .layer(axum::middleware::from_fn(oauth::security_headers))
}

const MCP_INSTRUCTIONS: &str = "Search foods using Crumbs full-text expressions. Results provide the best matches independently in common, branded, and custom datasets, including nutrition data and optional notes. Choose which foods to use. Manage custom foods with the custom-food tools; changes require crumbs:write.";
