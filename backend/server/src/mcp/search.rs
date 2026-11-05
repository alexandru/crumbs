//! MCP search tool schema and invocation.

use crumbs_database::{FoodReader, SearchError};
use rmcp::ErrorData;
use rmcp::model::{CallToolResponse, CallToolResult, Tool, ToolAnnotations};
use serde_json::{Map, Value, json};

use crate::search::{self, SearchRequest};

pub(super) const NAME: &str = "search_foods";

pub(super) async fn call<S: FoodReader + ?Sized>(
    reader: &S,
    arguments: Option<Map<String, Value>>,
) -> Result<CallToolResponse, ErrorData> {
    let request =
        match serde_json::from_value::<SearchRequest>(Value::Object(arguments.unwrap_or_default()))
        {
            Ok(request) => request,
            Err(error) => return Ok(tool_error(error.to_string()).into()),
        };
    match search::execute(reader, request).await {
        Ok(response) => {
            let value = serde_json::to_value(response)
                .map_err(|_| ErrorData::internal_error("food search serialization failed", None))?;
            Ok(CallToolResult::structured(value).into())
        }
        Err(SearchError::InvalidQuery(message)) => Ok(tool_error(message).into()),
        Err(SearchError::Storage(_)) => Ok(tool_error("food search failed").into()),
    }
}

pub(super) fn tool() -> Tool {
    let schema = Map::from_iter([
        ("type".to_owned(), json!("object")),
        ("required".to_owned(), json!(["query"])),
        (
            "properties".to_owned(),
            json!({
                "query": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Nonblank Crumbs full-text expression. Terms separated by spaces use AND. Supports quoted phrases, prefix matching with a trailing * (for example, chick*), AND/OR/NOT, parentheses, and field selectors for description, category, tags, source, and food_type. Terms are case-insensitive. Examples: chicken breast, \"chicken breast\", chick*, chicken OR turkey, description:chicken, category:dairy, tags:organic, source:USDA, chicken source:USDA, food_type:USDA_Foundation, chicken food_type:USDA_Foundation, food_type:USDA_Branded, food_type:USDA_SR_Legacy, food_type:USDA_Survey_FNDDS. USDA source values are stored as usda. Clients must quote values requiring quotes, such as values containing spaces, hyphens, or slashes. The server passes expressions to search unchanged and does not add quotes. No matches return an empty foods list; invalid expressions return an error."
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
        NAME,
        Some("Search foods with Crumbs full-text expressions over descriptions, categories, tags, source, and food type. Results include full nutrition data and are ordered by best-effort relevance, then food ID. No matches return an empty foods list. Invalid search expressions return an error. There is no pagination.".into()),
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
