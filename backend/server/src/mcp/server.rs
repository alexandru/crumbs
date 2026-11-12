//! MCP server protocol and tool dispatch.

use std::collections::HashMap;
use std::sync::Arc;

use rmcp::handler::server::tool::ToolCallContext;
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
use tracing::Instrument;

use crate::observability::ToolOutcome;

use super::{McpAuthorization, ToolRegistration};

pub(crate) struct McpServer<S> {
    reader: Arc<S>,
    tools: Arc<HashMap<String, RegisteredTool<S>>>,
    instructions: &'static str,
}

impl<S> ServerHandler for McpServer<S>
where
    S: Send + Sync + 'static,
{
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("crumbs", env!("CARGO_PKG_VERSION")))
            .with_instructions(self.instructions)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut tools: Vec<_> = self.tools.values().collect();
        tools.sort_by_key(|tool| tool.ordinal);
        let tools = tools
            .into_iter()
            .map(|tool| tool.registration.route.attr.clone())
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools
            .get(name)
            .map(|tool| tool.registration.route.attr.clone())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let registered_tool = self.tools.get(request.name.as_ref());
        let tool = registered_tool.map_or("<unknown>", |tool| {
            tool.registration.route.attr.name.as_ref()
        });
        let span = tracing::info_span!("mcp_tool", tool);
        let instrument_span = span.clone();
        async move {
            let lifecycle = crate::observability::Lifecycle::start(span);
            let Some(registered_tool) = registered_tool else {
                lifecycle.finish(ToolOutcome::UnknownTool, tool);
                return Err(ErrorData::new(
                    ErrorCode::METHOD_NOT_FOUND,
                    "unknown tool",
                    None,
                ));
            };

            let required_scope = registered_tool.registration.required_scope;
            let authorization = context
                .extensions
                .get::<axum::http::request::Parts>()
                .and_then(|parts| parts.extensions.get::<McpAuthorization>());
            let permitted =
                authorization.is_some_and(|authorization| authorization.permits(required_scope));
            if !permitted {
                lifecycle.finish(ToolOutcome::InsufficientScope, tool);
                return Ok(rmcp::model::CallToolResult::structured_error(json!({
                    "error": "insufficient_scope",
                    "required_scope": required_scope.as_str(),
                }))
                .into());
            }

            let result = (registered_tool.registration.route.call)(ToolCallContext::new(
                &self.reader,
                request,
                context,
            ))
            .await;
            let outcome = match &result {
                Ok(CallToolResponse::Complete(result)) if result.is_error == Some(true) => {
                    ToolOutcome::Error
                }
                Ok(CallToolResponse::Complete(_)) => ToolOutcome::Success,
                Ok(CallToolResponse::InputRequired(_)) => ToolOutcome::InputRequired,
                Ok(CallToolResponse::Task(_)) => ToolOutcome::TaskAccepted,
                Ok(_) => ToolOutcome::Other,
                Err(_) => ToolOutcome::ProtocolError,
            };
            lifecycle.finish(outcome, tool);
            result
        }
        .instrument(instrument_span)
        .await
    }
}

pub(crate) fn service<S>(
    reader: Arc<S>,
    config: StreamableHttpServerConfig,
    tools: impl IntoIterator<Item = ToolRegistration<S>>,
    instructions: &'static str,
) -> StreamableHttpService<McpServer<S>, LocalSessionManager>
where
    S: Send + Sync + 'static,
{
    let tools = Arc::new(
        tools
            .into_iter()
            .enumerate()
            .map(|(ordinal, registration)| {
                let name = registration.route.attr.name.to_string();
                (
                    name,
                    RegisteredTool {
                        registration,
                        ordinal,
                    },
                )
            })
            .collect(),
    );
    StreamableHttpService::new(
        move || {
            Ok(McpServer {
                reader: Arc::clone(&reader),
                tools: Arc::clone(&tools),
                instructions,
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

struct RegisteredTool<S> {
    registration: ToolRegistration<S>,
    ordinal: usize,
}

#[cfg(test)]
#[path = "../../unit-tests/mcp/server.rs"]
mod tests;
