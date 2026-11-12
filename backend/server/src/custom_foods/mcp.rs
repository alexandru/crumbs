//! MCP schemas and invocation for custom-food management.

use std::sync::Arc;

use crumbs_database::foods::{CustomFoodInput, CustomFoodStore};
use crumbs_database::identity::OAuthScope;
use rmcp::ErrorData;
use rmcp::handler::server::router::tool::ToolRoute;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolResponse, CallToolResult, Tool, ToolAnnotations};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::mcp::ToolRegistration;

use super::model::{FoodIdParameter, ListRequest, ListResponse};

pub(crate) fn mcp_tools<S>() -> impl Iterator<Item = ToolRegistration<S>>
where
    S: CustomFoodStore + Send + Sync + 'static,
{
    [
        CustomFoodTool::List,
        CustomFoodTool::Get,
        CustomFoodTool::Create,
        CustomFoodTool::Update,
        CustomFoodTool::Delete,
    ]
    .into_iter()
    .map(|tool| {
        let required_scope = if tool.requires_write() {
            OAuthScope::Write
        } else {
            OAuthScope::Read
        };
        let route: ToolRoute<Arc<S>> =
            ToolRoute::new_dyn(tool.tool(), move |context: ToolCallContext<'_, Arc<S>>| {
                Box::pin(
                    async move { tool.call(context.service.as_ref(), context.arguments).await },
                )
            });
        ToolRegistration::new(route, required_scope)
    })
}

#[derive(Clone, Copy)]
enum CustomFoodTool {
    List,
    Get,
    Create,
    Update,
    Delete,
}

impl CustomFoodTool {
    const fn requires_write(self) -> bool {
        matches!(self, Self::Create | Self::Update | Self::Delete)
    }

    fn tool(self) -> Tool {
        let description = match self {
            Self::List => {
                r#"List custom foods by food ID, including nutrition and optional notes.

Example arguments:
```json
{
  "limit": 20
}
```

- Omit limit for 20; supplied limits are clamped to 20 through 1000.
- There is no pagination. Requires crumbs:read."#
            }
            Self::Get => {
                r#"Get a custom food by its local food ID, including nutrition and optional notes.

Example arguments:
```json
{
  "id": 123
}
```

- Imported or missing IDs return an error. Requires crumbs:read."#
            }
            Self::Create => {
                r#"Create a custom food with optional nutrition and measured servings.

Example arguments:
```json
{
  "food": {
    "description": "Oat drink",
    "category": "Drinks",
    "notes": "Oats and water",
    "parsed_data": {
      "nutrition": {
        "per": "100 ml",
        "energy": "45 kcal",
        "fat": {"total": "1.5 g"},
        "protein": "1 g"
      },
      "serving_sizes": [{"name": "cup", "quantity": "250 ml"}]
    }
  }
}
```

- Use unit-bearing decimal strings, not objects or numbers, for energy, nutrients, and serving quantities.
- Nutrition is per 100 g or 100 ml; omitted nutrient amounts are unknown. Omit optional fields when unknown.
- Crumbs assigns source, dataset, and IDs. Requires crumbs:write."#
            }
            Self::Update => {
                r#"Replace a custom food's description, category, notes, nutrition, and servings.

Example arguments:
```json
{
  "id": 123,
  "food": {
    "description": "Oat drink",
    "category": "Drinks",
    "notes": "Oats and water",
    "parsed_data": {
      "nutrition": {
        "per": "100 ml",
        "energy": "45 kcal",
        "fat": {"total": "1.5 g"},
        "protein": "1 g"
      },
      "serving_sizes": [{"name": "cup", "quantity": "250 ml"}]
    }
  }
}
```

- Use unit-bearing decimal strings, not objects or numbers, for energy, nutrients, and serving quantities.
- Nutrition is per 100 g or 100 ml; omitted nutrient amounts are unknown.
- Omitted optional fields are cleared. Local and source identity and tag assignments are preserved.
- Imported or missing IDs return an error. Requires crumbs:write."#
            }
            Self::Delete => {
                r#"Permanently delete a custom food and its tag assignments.

Example arguments:
```json
{
  "id": 123
}
```

- Imported or missing IDs return an error. Requires crumbs:write."#
            }
        };
        Tool::new_with_raw(self.name(), Some(description.into()), self.schema()).with_annotations(
            ToolAnnotations::new()
                .read_only(!self.requires_write())
                .destructive(matches!(self, Self::Update | Self::Delete))
                .open_world(false),
        )
    }

    async fn call<S: CustomFoodStore + ?Sized>(
        self,
        store: &S,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResponse, ErrorData> {
        match self
            .execute(store, Value::Object(arguments.unwrap_or_default()))
            .await
        {
            Ok(value) => Ok(CallToolResult::structured(value).into()),
            Err(ToolFailure::InvalidInput(message)) => Ok(tool_error(message).into()),
            Err(ToolFailure::NotFound) => Ok(tool_error("custom food not found").into()),
            Err(ToolFailure::Storage) => Ok(tool_error("custom food operation failed").into()),
            Err(ToolFailure::Serialization) => Err(ErrorData::internal_error(
                "custom food serialization failed",
                None,
            )),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::List => "list_custom_foods",
            Self::Get => "get_custom_food",
            Self::Create => "create_custom_food",
            Self::Update => "update_custom_food",
            Self::Delete => "delete_custom_food",
        }
    }

    fn schema(self) -> Map<String, Value> {
        let mut schema = Map::from_iter([
            ("type".to_owned(), json!("object")),
            ("additionalProperties".to_owned(), json!(false)),
        ]);
        let id = json!({"type": "integer", "minimum": 1, "description": "Local Crumbs food ID."});
        match self {
            Self::List => {
                schema.insert(
                    "properties".to_owned(),
                    json!({
                        "limit": {
                            "type": "integer",
                            "default": 20,
                            "description": "Maximum foods returned; clamped to 20 through 1000."
                        }
                    }),
                );
            }
            Self::Get | Self::Delete => {
                schema.insert("required".to_owned(), json!(["id"]));
                schema.insert("properties".to_owned(), json!({"id": id}));
            }
            Self::Create => {
                schema.insert("required".to_owned(), json!(["food"]));
                schema.insert(
                    "properties".to_owned(),
                    json!({"food": {"$ref": "#/$defs/food"}}),
                );
                schema.insert("$defs".to_owned(), food_definitions());
            }
            Self::Update => {
                schema.insert("required".to_owned(), json!(["id", "food"]));
                schema.insert(
                    "properties".to_owned(),
                    json!({
                        "id": id,
                        "food": {"$ref": "#/$defs/food"}
                    }),
                );
                schema.insert("$defs".to_owned(), food_definitions());
            }
        }
        schema
    }

    async fn execute<S: CustomFoodStore + ?Sized>(
        self,
        store: &S,
        arguments: Value,
    ) -> Result<Value, ToolFailure> {
        match self {
            Self::List => {
                let request: ListRequest = parse(arguments)?;
                let foods = store
                    .list_custom_foods(request.limit)
                    .await
                    .map_err(|_| ToolFailure::Storage)?;
                serialize(ListResponse { foods })
            }
            Self::Get => {
                let request: IdRequest = parse(arguments)?;
                let food = store
                    .custom_food(request.id.food_id())
                    .await
                    .map_err(|_| ToolFailure::Storage)?
                    .ok_or(ToolFailure::NotFound)?;
                serialize(food)
            }
            Self::Create => {
                let request: CreateRequest = parse(arguments)?;
                let food = store
                    .create_custom_food(&request.food)
                    .await
                    .map_err(|_| ToolFailure::Storage)?;
                serialize(food)
            }
            Self::Update => {
                let request: UpdateRequest = parse(arguments)?;
                let food = store
                    .update_custom_food(request.id.food_id(), &request.food)
                    .await
                    .map_err(|_| ToolFailure::Storage)?
                    .ok_or(ToolFailure::NotFound)?;
                serialize(food)
            }
            Self::Delete => {
                let request: IdRequest = parse(arguments)?;
                let id = request.id.food_id();
                if !store
                    .delete_custom_food(id)
                    .await
                    .map_err(|_| ToolFailure::Storage)?
                {
                    return Err(ToolFailure::NotFound);
                }
                Ok(json!({"id": id, "deleted": true}))
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdRequest {
    id: FoodIdParameter,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    food: CustomFoodInput,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    id: FoodIdParameter,
    food: CustomFoodInput,
}

enum ToolFailure {
    InvalidInput(String),
    NotFound,
    Storage,
    Serialization,
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, ToolFailure> {
    serde_json::from_value(value).map_err(|error| ToolFailure::InvalidInput(error.to_string()))
}

fn serialize<T: Serialize>(value: T) -> Result<Value, ToolFailure> {
    serde_json::to_value(value).map_err(|_| ToolFailure::Serialization)
}

fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(json!({"error": message.into()}))
}

fn food_definitions() -> Value {
    json!({
        "food": {
            "type": "object",
            "additionalProperties": false,
            "required": ["description"],
            "properties": {
                "description": {"type": "string", "minLength": 1, "pattern": "\\S", "description": "Nonblank food name or description without NUL characters."},
                "category": {"type": ["string", "null"]},
                "notes": {"type": ["string", "null"], "description": "Free text such as ingredients. Returned with the food but excluded from full-text search."},
                "parsed_data": {"$ref": "#/$defs/food_data", "description": "Optional object containing nutrition and serving_sizes."}
            }
        },
        "food_data": {
            "type": "object",
            "description": "Nutrition applies per 100 g or 100 ml. Omitted nutrient amounts are unknown. Amounts are unit-bearing decimal strings, such as 7 g or 120 kcal.",
            "properties": {
                "nutrition": {"anyOf": [{"$ref": "#/$defs/nutrition"}, {"type": "null"}], "description": "Optional nutrition object; use null when nutrition is unknown."},
                "serving_sizes": {"type": "array", "items": {"$ref": "#/$defs/serving"}, "description": "Named measured servings, each with name and quantity."}
            }
        },
        "mass": {"type": ["string", "null"], "description": "Nonnegative reported mass with a unit, for example 7 g. Use a string to preserve decimal precision."},
        "nutrition": {
            "type": "object",
            "required": ["per"],
            "properties": {
                "per": {"type": "string", "enum": ["100 g", "100 ml"]},
                "energy": {"type": ["string", "null"], "description": "Reported energy with a unit, for example 120 kcal or 500 kJ."},
                "fat": {"$ref": "#/$defs/fat", "description": "Object with optional mass fields such as total and saturated; each value is a unit-bearing string."},
                "carbohydrates": {"$ref": "#/$defs/carbohydrates", "description": "Object with optional mass fields and a sugars object; each mass value is a unit-bearing string."},
                "fibre": {"$ref": "#/$defs/mass"},
                "protein": {"$ref": "#/$defs/mass"},
                "sodium": {"$ref": "#/$defs/mass"},
                "salt": {"$ref": "#/$defs/mass"},
                "cholesterol": {"$ref": "#/$defs/mass"}
            }
        },
        "fat": mass_properties(&["total", "saturated", "monounsaturated", "polyunsaturated", "trans"]),
        "carbohydrates": {
            "type": "object",
            "properties": {
                "including_fibre": {"$ref": "#/$defs/mass"},
                "excluding_fibre": {"$ref": "#/$defs/mass"},
                "starch": {"$ref": "#/$defs/mass"},
                "sugars": {"$ref": "#/$defs/sugars"}
            }
        },
        "sugars": mass_properties(&["total", "added", "lactose"]),
        "serving": {
            "type": "object",
            "required": ["name", "quantity"],
            "properties": {
                "name": {"type": "string"},
                "quantity": {"type": "string", "description": "Positive measured mass or volume, for example 30 g or 250 ml."}
            }
        }
    })
}

fn mass_properties(names: &[&str]) -> Value {
    let properties: Map<String, Value> = names
        .iter()
        .map(|name| ((*name).to_owned(), json!({"$ref": "#/$defs/mass"})))
        .collect();
    json!({"type": "object", "properties": properties})
}

#[cfg(test)]
#[path = "../../unit-tests/custom_foods/mcp.rs"]
mod tests;
