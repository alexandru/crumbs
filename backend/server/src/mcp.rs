//! Hosted MCP adapter for the shared food search operation.

use std::sync::Arc;

use crumbs_database::FoodReader;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ErrorCode, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData, RoleServer, ServerHandler};

mod search;

pub(crate) struct FoodSearchServer<S> {
    reader: Arc<S>,
}

impl<S> ServerHandler for FoodSearchServer<S>
where
    S: FoodReader + Send + 'static,
{
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("crumbs", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Search foods using Crumbs full-text expressions. Results include full nutrition data.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![search::tool()]))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        (name == search::NAME).then(search::tool)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name != search::NAME {
            return Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                "unknown tool",
                None,
            ));
        }

        search::call(self.reader.as_ref(), request.arguments).await
    }
}

pub(crate) fn service<S>(
    reader: Arc<S>,
    config: StreamableHttpServerConfig,
) -> StreamableHttpService<FoodSearchServer<S>, LocalSessionManager>
where
    S: FoodReader + Send + 'static,
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
