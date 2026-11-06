//! MCP search tool schema and invocation.

use std::sync::Arc;

use crumbs_database::oauth::OAuthScope;
use crumbs_database::{FoodReader, SearchError};
use rmcp::ErrorData;
use rmcp::handler::server::router::tool::ToolRoute;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolResponse, CallToolResult, Tool, ToolAnnotations};
use serde_json::{Map, Value, json};

use super::SearchRequest;
use crate::mcp::ToolRegistration;

pub(crate) fn mcp_tools<S>() -> impl Iterator<Item = ToolRegistration<S>>
where
    S: FoodReader + Send + Sync + 'static,
{
    let route: ToolRoute<Arc<S>> =
        ToolRoute::new_dyn(tool(), |context: ToolCallContext<'_, Arc<S>>| {
            Box::pin(async move { call(context.service.as_ref(), context.arguments).await })
        });
    std::iter::once(ToolRegistration::new(route, OAuthScope::Read))
}

const NAME: &str = "search_foods";

async fn call<S: FoodReader + ?Sized>(
    reader: &S,
    arguments: Option<Map<String, Value>>,
) -> Result<CallToolResponse, ErrorData> {
    let request =
        match serde_json::from_value::<SearchRequest>(Value::Object(arguments.unwrap_or_default()))
        {
            Ok(request) => request,
            Err(error) => return Ok(tool_error(error.to_string()).into()),
        };
    match super::execute(reader, request).await {
        Ok(response) => {
            let value = serde_json::to_value(response)
                .map_err(|_| ErrorData::internal_error("food search serialization failed", None))?;
            Ok(CallToolResult::structured(value).into())
        }
        Err(SearchError::InvalidQuery(message)) => Ok(tool_error(message).into()),
        Err(SearchError::Storage(_)) => Ok(tool_error("food search failed").into()),
    }
}

fn tool() -> Tool {
    let schema = Map::from_iter([
        ("type".to_owned(), json!("object")),
        ("required".to_owned(), json!(["query"])),
        (
            "properties".to_owned(),
            json!({
                "query": {
                    "type": "string",
                    "minLength": 1,
                    "description": r#"Nonblank Crumbs full-text expression.

Syntax:
- Terms separated by spaces use AND.
- Quoted phrases match exact phrases.
- A trailing `*` performs prefix matching, for example `chick*`.
- Use `AND`, `OR`, `NOT`, and parentheses to combine expressions.
- Terms are case-insensitive.

Search fields:
- `description`: food descriptions.
- `category`: food categories.
- `tags`: assigned tags.
- `source`: data provider. USDA source values are stored as `usda`.
- `dataset`: source-independent search classification: `common`, `branded`, or `custom`.

Examples:
- `chicken breast`
- `"chicken breast"`
- `chick*`
- `chicken OR turkey`
- `description:chicken`, `category:dairy`, `tags:organic`
- `source:usda`, `chicken source:usda`
- `dataset:common`, `chicken dataset:common`
- `dataset:branded`, `dataset:custom`

Clients must quote values requiring quotes, such as values containing spaces, hyphens, or slashes. Results contain separate `common`, `branded`, and `custom` lists, each with its own limit. No matches return three empty lists. Food notes are returned but are not searched. Invalid expressions return an error."#
                },
                "limit": {
                    "type": "integer",
                    "default": 20,
                    "description": "Maximum foods returned per dataset. Defaults to 20; supplied values are clamped to 20 through 1000 independently for common, branded, and custom."
                }
            }),
        ),
    ]);
    Tool::new_with_raw(
        NAME,
        Some("Search foods with Crumbs full-text expressions over descriptions, categories, tags, source, and dataset. Results contain common, branded, and custom lists with independent limits, full nutrition data, and optional notes. Within each dataset, results use best-effort relevance, then food ID. Choose the appropriate food across datasets. No matches return three empty lists. Invalid search expressions return an error. There is no pagination.".into()),
        schema,
    )
    .with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .open_world(false),
    )
}

fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(json!({ "error": message.into() }))
}
