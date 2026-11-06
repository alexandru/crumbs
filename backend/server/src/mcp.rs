//! Hosted MCP tools for food search and custom-food management.

use std::sync::Arc;

use crumbs_database::{CustomFoodStore, FoodReader};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ErrorCode, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::json;

use crate::oauth::FoodScopes;

mod authorization;
mod custom_foods;
mod search;

pub(crate) use authorization::McpAuthorization;

pub(crate) struct FoodSearchServer<S> {
    reader: Arc<S>,
}

impl<S> ServerHandler for FoodSearchServer<S>
where
    S: FoodReader + CustomFoodStore + Send + 'static,
{
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("crumbs", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Search foods using Crumbs full-text expressions. Results provide the best matches independently in common, branded, and custom datasets, including nutrition data and optional notes. Choose which foods to use. Manage custom foods with the custom-food tools; changes require crumbs:write.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = std::iter::once(search::tool())
            .chain(custom_foods::CustomFoodTool::tools())
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        if name == search::NAME {
            Some(search::tool())
        } else {
            custom_foods::CustomFoodTool::from_name(name).map(|tool| tool.tool())
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let custom_tool = custom_foods::CustomFoodTool::from_name(&request.name);
        if request.name != search::NAME && custom_tool.is_none() {
            return Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                "unknown tool",
                None,
            ));
        }

        let required = if custom_tool.is_some_and(|tool| tool.requires_write()) {
            FoodScopes::Write
        } else {
            FoodScopes::Read
        };
        let authorization = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<McpAuthorization>());
        let permitted = authorization.is_some_and(|authorization| authorization.permits(required));
        if !permitted {
            return Ok(rmcp::model::CallToolResult::structured_error(json!({
                "error": "insufficient_scope",
                "required_scope": required.as_str(),
            }))
            .into());
        }

        match custom_tool {
            Some(tool) => tool.call(self.reader.as_ref(), request.arguments).await,
            None => search::call(self.reader.as_ref(), request.arguments).await,
        }
    }
}

pub(crate) fn service<S>(
    reader: Arc<S>,
    config: StreamableHttpServerConfig,
) -> StreamableHttpService<FoodSearchServer<S>, LocalSessionManager>
where
    S: FoodReader + CustomFoodStore + Send + 'static,
{
    StreamableHttpService::new(
        move || {
            Ok(FoodSearchServer {
                reader: Arc::clone(&reader),
            })
        },
        Arc::new(LocalSessionManager::default()),
        config,
    )
}

pub(crate) fn config() -> StreamableHttpServerConfig {
    StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        // This public hosted service accepts its deployment's Host header.
        // Browser origins are rejected; hosted harnesses do not require one.
        .disable_allowed_hosts()
        .enforce_origin_validation()
}

#[cfg(test)]
#[path = "../unit-tests/mcp.rs"]
mod tests;
