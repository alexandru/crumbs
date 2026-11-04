//! Hosted MCP adapter for the shared food search operation.

use std::sync::Arc;

use axum::Router;
use crumbs_database::{FoodReader, SearchError};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ErrorCode, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Map, Value, json};

use crate::search::{self, SearchRequest};

pub(crate) fn router<S>(reader: Arc<S>, config: StreamableHttpServerConfig) -> Router
where
    S: FoodReader + Send + 'static,
{
    let service = StreamableHttpService::new(
        move || {
            Ok(FoodSearchServer {
                reader: Arc::clone(&reader),
            })
        },
        Arc::new(LocalSessionManager::default()),
        config,
    );
    Router::new().nest_service("/mcp", service)
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

struct FoodSearchServer<S> {
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
                "Search foods using SQLite FTS5 expressions. Results include full nutrition data.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![search_tool()]))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        (name == SEARCH_TOOL_NAME).then(search_tool)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name != SEARCH_TOOL_NAME {
            return Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                "unknown tool",
                None,
            ));
        }
        let request = match serde_json::from_value::<SearchRequest>(Value::Object(
            request.arguments.unwrap_or_default(),
        )) {
            Ok(request) => request,
            Err(error) => return Ok(tool_error(error.to_string()).into()),
        };
        match search::execute(self.reader.as_ref(), request).await {
            Ok(response) => {
                let value = serde_json::to_value(response).map_err(|_| {
                    ErrorData::internal_error("food search serialization failed", None)
                })?;
                Ok(CallToolResult::structured(value).into())
            }
            Err(SearchError::InvalidQuery(message)) => Ok(tool_error(message).into()),
            Err(SearchError::Storage(_)) => Ok(tool_error("food search failed").into()),
        }
    }
}

const SEARCH_TOOL_NAME: &str = "search_foods";

fn search_tool() -> Tool {
    let schema = Map::from_iter([
        ("type".to_owned(), json!("object")),
        ("required".to_owned(), json!(["query"])),
        (
            "properties".to_owned(),
            json!({
                "query": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Nonblank SQLite FTS5 expression over description, category, and tags. Examples: chicken breast, \"chicken breast\", chick*, chicken OR turkey, tags:organic. Matches ignore case and Latin diacritics."
                },
                "limit": {
                    "type": "integer",
                    "default": 20,
                    "description": "Maximum foods returned. Defaults to 20; supplied values are clamped to 20 through 1000."
                }
            }),
        ),
    ]);
    Tool::new_with_raw(
        SEARCH_TOOL_NAME,
        Some("Search food descriptions, categories, and tags. Return matching foods with full nutrition data, ordered by relevance then food ID. No pagination.".into()),
        schema,
    )
    .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).open_world(false))
}

fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(json!({ "error": message.into() }))
}

#[cfg(test)]
#[path = "../unit-tests/mcp.rs"]
mod tests;
