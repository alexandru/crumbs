//! MCP schemas and invocation for profile logs.

use std::sync::Arc;

use axum::http::request::Parts;
use crumbs_database::identity::{OAuthScope, UserId, UserSession};
use crumbs_database::profile_logs::{
    CreateLog, LogFilter, PatchLog, ProfileLogError, ProfileLogId, ProfileLogs,
};
use crumbs_database::profiles::ProfileId;
use rmcp::ErrorData;
use rmcp::handler::server::router::tool::ToolRoute;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolResponse, CallToolResult, Tool, ToolAnnotations};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::mcp::ToolRegistration;

pub(crate) fn mcp_tools<S>() -> impl Iterator<Item = ToolRegistration<S>>
where
    S: ProfileLogs + Send + Sync + 'static,
{
    [
        ProfileLogTool::List,
        ProfileLogTool::Get,
        ProfileLogTool::Create,
        ProfileLogTool::Update,
        ProfileLogTool::Delete,
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
                Box::pin(async move {
                    let user = context
                        .request_context()
                        .extensions
                        .get::<Parts>()
                        .and_then(|parts| parts.extensions.get::<UserSession>())
                        .map(|session| session.user_id);
                    match user {
                        Some(user) => {
                            tool.call(context.service.as_ref(), user, context.arguments)
                                .await
                        }
                        None => Ok(tool_error(
                            "unauthorized",
                            "authenticated user session is required",
                        )
                        .into()),
                    }
                })
            });
        ToolRegistration::new(route, required_scope)
    })
}

#[derive(Clone, Copy)]
enum ProfileLogTool {
    List,
    Get,
    Create,
    Update,
    Delete,
}

impl ProfileLogTool {
    const fn requires_write(self) -> bool {
        matches!(self, Self::Create | Self::Update | Self::Delete)
    }

    fn tool(self) -> Tool {
        let description = match self {
            Self::List => {
                r#"List food and biometric logs for one profile owned by the authenticated account.

Example arguments:
```json
{
  "profile_id": 1,
  "filter": {
    "from": "2026-11-01",
    "to": "2026-11-12",
    "entry_type": "food"
  }
}
```

- All filter fields are optional. Omit filter to list all entry types.
- Dates are inclusive YYYY-MM-DD strings.
- For weight logs, set entry_type to "biometric" and subtype to "weight".
- Results contain at most 100 logs, ordered by ID descending.
- For the next page, set filter.last_id to the last returned ID. Only smaller IDs are returned.

Requires crumbs:read."#
            }
            Self::Get => {
                r#"Get one log from a profile owned by the authenticated account.

Example arguments:
```json
{
  "profile_id": 1,
  "id": 42
}
```

- profile_id identifies the profile.
- id identifies the log in that profile.
- Both IDs are positive integers.

Requires crumbs:read."#
            }
            Self::Create => {
                r#"Create one food or weight log for a profile owned by the authenticated account.

Food arguments:
```json
{
  "profile_id": 1,
  "log": {
    "day": "2026-11-12",
    "entry": {
      "type": "food",
      "food_id": 123,
      "amount": "150",
      "serving_size": "g"
    }
  }
}
```

Named serving entry (inside log.entry):
```json
{
  "type": "food",
  "food_id": 123,
  "amount": "1",
  "serving_size": "one cup"
}
```

Weight arguments:
```json
{
  "profile_id": 1,
  "log": {
    "day": "2026-11-12",
    "entry": {
      "type": "biometric",
      "measurement": {
        "type": "weight",
        "value": "90 kg"
      }
    }
  }
}
```

- Food amount is a positive decimal string without a unit.
- Put the unit or exact named serving in the separate serving_size string.
- Use an existing food_id from food search or custom-food tools.
- Units are kg, g, mg, µg, l, or ml and must match the food's nutrition basis.
- Named servings must match a serving defined by that food, including case.
- The server copies calories, available nutrients, and description into a stored snapshot.
- If calories are missing, edit the food or create a food with calories before logging it.
- day is a required YYYY-MM-DD string.
- Optional log.time is a profile-local time string such as "14:30". Omit it or use null for the current local time in the profile timezone, including for past dates.

Requires crumbs:write."#
            }
            Self::Update => {
                r#"Update selected fields of one log without changing its profile or entry type.

Food arguments:
```json
{
  "profile_id": 1,
  "id": 42,
  "patch": {
    "entry": {
      "type": "food",
      "amount": "200",
      "serving_size": "g"
    }
  }
}
```

Weight arguments:
```json
{
  "profile_id": 1,
  "id": 42,
  "patch": {
    "entry": {
      "type": "biometric",
      "measurement": {
        "type": "weight",
        "value": "89.5 kg"
      }
    }
  }
}
```

Date/time arguments:
```json
{
  "profile_id": 1,
  "id": 42,
  "patch": {
    "day": "2026-11-12",
    "time": "14:30"
  }
}
```

- Omit fields to keep their stored values. Null is invalid.
- For food entries, amount is a positive decimal string without a unit.
- serving_size is a separate unit or exact named serving string.
- food_id, amount, and serving_size are independently optional inside patch.entry.
- type is required when entry is supplied.
- Supplied food input fields rebuild the snapshot from the current food record.
- Date/time-only changes preserve the snapshot.

Requires crumbs:write."#
            }
            Self::Delete => {
                r#"Delete one log from a profile owned by the authenticated account.

Example arguments:
```json
{
  "profile_id": 1,
  "id": 42
}
```

- profile_id identifies the profile.
- id identifies the log in that profile.
- Both IDs are positive integers.
- Deleting a weight log recalculates the profile's current weight.

Requires crumbs:write."#
            }
        };
        Tool::new_with_raw(self.name(), Some(description.into()), self.schema()).with_annotations(
            ToolAnnotations::new()
                .read_only(!self.requires_write())
                .destructive(matches!(self, Self::Update | Self::Delete))
                .open_world(false),
        )
    }

    const fn name(self) -> &'static str {
        match self {
            Self::List => "list_profile_logs",
            Self::Get => "get_profile_log",
            Self::Create => "create_profile_log",
            Self::Update => "update_profile_log",
            Self::Delete => "delete_profile_log",
        }
    }

    fn schema(self) -> Map<String, Value> {
        let mut schema = Map::from_iter([
            ("type".to_owned(), json!("object")),
            ("additionalProperties".to_owned(), json!(false)),
        ]);
        let profile_id =
            json!({"type": "integer", "minimum": 1, "description": "Positive profile ID."});
        let log_id =
            json!({"type": "integer", "minimum": 1, "description": "Positive profile log ID."});
        match self {
            Self::List => {
                schema.insert("required".to_owned(), json!(["profile_id"]));
                schema.insert(
                    "properties".to_owned(),
                    json!({
                        "profile_id": profile_id,
                        "filter": {"anyOf": [{"$ref": "#/$defs/log_filter"}, {"type": "null"}]}
                    }),
                );
                schema.insert("$defs".to_owned(), definitions());
            }
            Self::Get | Self::Delete => {
                schema.insert("required".to_owned(), json!(["profile_id", "id"]));
                schema.insert(
                    "properties".to_owned(),
                    json!({"profile_id": profile_id, "id": log_id}),
                );
            }
            Self::Create => {
                schema.insert("required".to_owned(), json!(["profile_id", "log"]));
                schema.insert(
                    "properties".to_owned(),
                    json!({
                        "profile_id": profile_id,
                        "log": {"$ref": "#/$defs/create_log"}
                    }),
                );
                schema.insert("$defs".to_owned(), definitions());
            }
            Self::Update => {
                schema.insert("required".to_owned(), json!(["profile_id", "id", "patch"]));
                schema.insert(
                    "properties".to_owned(),
                    json!({
                        "profile_id": profile_id,
                        "id": log_id,
                        "patch": {"$ref": "#/$defs/patch_log"}
                    }),
                );
                schema.insert("$defs".to_owned(), definitions());
            }
        }
        schema
    }

    async fn call<S: ProfileLogs + ?Sized>(
        self,
        store: &S,
        user: UserId,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResponse, ErrorData> {
        match self
            .execute(store, user, Value::Object(arguments.unwrap_or_default()))
            .await
        {
            Ok(value) => Ok(CallToolResult::structured(value).into()),
            Err(ToolFailure::Profile(error)) => Ok(profile_log_tool_error(error).into()),
            Err(ToolFailure::NotFound) => {
                Ok(tool_error("not_found", "profile log not found").into())
            }
            Err(ToolFailure::InvalidInput(message)) => {
                Ok(tool_error("invalid_input", message).into())
            }
            Err(ToolFailure::Serialization) => Err(ErrorData::internal_error(
                "profile log serialization failed",
                None,
            )),
        }
    }

    async fn execute<S: ProfileLogs + ?Sized>(
        self,
        store: &S,
        user: UserId,
        arguments: Value,
    ) -> Result<Value, ToolFailure> {
        match self {
            Self::List => {
                let request: ListRequest = parse(arguments)?;
                let filter = request.filter.unwrap_or_default();
                let page = store
                    .list_profile_logs(user, request.profile_id, &filter)
                    .await
                    .map_err(ToolFailure::Profile)?
                    .ok_or(ToolFailure::NotFound)?;
                serialize(page)
            }
            Self::Get => {
                let request: IdRequest = parse(arguments)?;
                let log = store
                    .profile_log(user, request.profile_id, request.id)
                    .await
                    .map_err(ToolFailure::Profile)?
                    .ok_or(ToolFailure::NotFound)?;
                serialize(log)
            }
            Self::Create => {
                let request: CreateRequest = parse(arguments)?;
                let log = store
                    .create_profile_log(user, request.profile_id, &request.log)
                    .await
                    .map_err(ToolFailure::Profile)?
                    .ok_or(ToolFailure::NotFound)?;
                serialize(log)
            }
            Self::Update => {
                let request: PatchRequest = parse(arguments)?;
                let log = store
                    .patch_profile_log(user, request.profile_id, request.id, &request.patch)
                    .await
                    .map_err(ToolFailure::Profile)?
                    .ok_or(ToolFailure::NotFound)?;
                serialize(log)
            }
            Self::Delete => {
                let request: IdRequest = parse(arguments)?;
                if !store
                    .delete_profile_log(user, request.profile_id, request.id)
                    .await
                    .map_err(ToolFailure::Profile)?
                {
                    return Err(ToolFailure::NotFound);
                }
                Ok(json!({"id": request.id.get(), "deleted": true}))
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdRequest {
    profile_id: ProfileId,
    id: ProfileLogId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    profile_id: ProfileId,
    log: CreateLog,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchRequest {
    profile_id: ProfileId,
    id: ProfileLogId,
    patch: PatchLog,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListRequest {
    profile_id: ProfileId,
    #[serde(default)]
    filter: Option<LogFilter>,
}

enum ToolFailure {
    InvalidInput(String),
    NotFound,
    Profile(ProfileLogError),
    Serialization,
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, ToolFailure> {
    serde_json::from_value(value).map_err(|error| ToolFailure::InvalidInput(error.to_string()))
}

fn serialize<T: Serialize>(value: T) -> Result<Value, ToolFailure> {
    serde_json::to_value(value).map_err(|_| ToolFailure::Serialization)
}

fn profile_log_tool_error(error: ProfileLogError) -> CallToolResult {
    let code = error.code();
    let message = match error {
        ProfileLogError::InvalidInput(message) => message,
        ProfileLogError::InvalidStoredData
        | ProfileLogError::Storage(_)
        | ProfileLogError::Clock(_) => "profile log operation failed".to_owned(),
    };
    tool_error(code, message)
}

fn tool_error(code: &'static str, message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(json!({"code": code, "error": message.into()}))
}

fn definitions() -> Value {
    json!({
        "create_log": {
            "type": "object",
            "additionalProperties": false,
            "required": ["day", "entry"],
            "properties": {
                "day": {"$ref": "#/$defs/log_day"},
                "time": {"anyOf": [{"$ref": "#/$defs/local_time"}, {"type": "null"}], "description": "Profile-local time. Omit or use null to resolve the current local time in the profile timezone."},
                "entry": {"$ref": "#/$defs/log_entry"}
            }
        },
        "patch_log": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "day": {"$ref": "#/$defs/log_day", "description": "Replace the occurrence day. Omission preserves the stored day."},
                "time": {"$ref": "#/$defs/local_time", "description": "Replace the occurrence time. Null is invalid; omission preserves the stored time."},
                "entry": {"$ref": "#/$defs/log_entry_patch", "description": "Patch the weight measurement or food input without changing the entry category. Food input fields refresh the snapshot; an empty food patch preserves it."}
            }
        },
        "log_filter": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "from": {"anyOf": [{"$ref": "#/$defs/log_day"}, {"type": "null"}], "description": "Inclusive lower day bound."},
                "to": {"anyOf": [{"$ref": "#/$defs/log_day"}, {"type": "null"}], "description": "Inclusive upper day bound."},
                "entry_type": {"anyOf": [{"type": "string", "enum": ["food", "biometric"]}, {"type": "null"}]},
                "subtype": {"anyOf": [{"type": "string", "enum": ["weight"]}, {"type": "null"}], "description": "Biometric subtype; when set to weight, requires entry_type=biometric."},
                "last_id": {"anyOf": [{"type": "integer", "minimum": 1}, {"type": "null"}], "description": "Exclusive cursor. Results are ordered by ID descending; next page uses id < last_id."}
            },
            "allOf": [
                {
                    "if": {"required": ["subtype"], "properties": {"subtype": {"const": "weight"}}},
                    "then": {"required": ["entry_type"], "properties": {"entry_type": {"const": "biometric"}}}
                }
            ],
            "description": "Date bounds are inclusive. Results are always ordered by ID descending in fixed pages of 100. No limit or sort-direction parameter is accepted."
        },
        "log_day": {"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$", "description": "Valid calendar day in YYYY-MM-DD form."},
        "local_time": {"type": "string", "pattern": "^([01][0-9]|2[0-3]):[0-5][0-9](?::[0-5][0-9](\\.[0-9]{1,3})?)?$", "description": "Profile-local time. Input accepts HH:MM, HH:MM:SS, or HH:MM:SS.f through HH:MM:SS.fff; responses serialize as HH:MM:SS.fff."},
        "log_entry": {
            "oneOf": [
                {"$ref": "#/$defs/biometric_log_entry"},
                {"$ref": "#/$defs/food_log_entry"}
            ]
        },
        "log_entry_patch": {
            "oneOf": [
                {"$ref": "#/$defs/biometric_log_entry"},
                {"$ref": "#/$defs/food_log_entry_patch"}
            ]
        },
        "biometric_log_entry": {
            "type": "object",
            "additionalProperties": false,
            "required": ["type", "measurement"],
            "properties": {
                "type": {"const": "biometric"},
                "measurement": {"$ref": "#/$defs/biometric_entry"}
            }
        },
        "food_log_entry": {
            "type": "object",
            "additionalProperties": false,
            "required": ["type", "food_id", "amount", "serving_size"],
            "properties": {
                "type": {"const": "food"},
                "food_id": {"type": "integer", "minimum": 1, "description": "Existing positive FoodId. The server validates the food and derives the snapshot."},
                "amount": {"type": "string", "pattern": "^(?:[1-9][0-9]*(?:\\.[0-9]+)?|0*\\.[0-9]*[1-9][0-9]*)$", "description": "Positive decimal string without a unit, such as \"300\" or \"1.5\". Set the unit or named serving in the sibling serving_size field."},
                "serving_size": {"type": "string", "minLength": 1, "pattern": "\\S", "description": "One of kg, g, mg, µg, l, ml, or one nonblank, case-sensitive named serving defined by this food. The dimension must match the food's nutrition basis; surrounding whitespace is ignored."}
            }
        },
        "food_log_entry_patch": {
            "type": "object",
            "additionalProperties": false,
            "required": ["type"],
            "properties": {
                "type": {"const": "food"},
                "food_id": {"type": "integer", "minimum": 1, "description": "Existing positive FoodId. Omit to keep the stored FoodId."},
                "amount": {"type": "string", "pattern": "^(?:[1-9][0-9]*(?:\\.[0-9]+)?|0*\\.[0-9]*[1-9][0-9]*)$", "description": "Positive decimal string without a unit, such as \"1.5\". Set the unit or named serving in the sibling serving_size field. Omit to keep the stored amount."},
                "serving_size": {"type": "string", "minLength": 1, "pattern": "\\S", "description": "kg, g, mg, µg, l, ml, or one nonblank, case-sensitive named serving defined by the food. Omit to keep the stored serving. Null is invalid."}
            },
            "description": "Fields are optional and null is invalid. An empty food patch preserves the existing snapshot. Supplying any input field rebuilds the snapshot from the current food record after the merged food ID, amount, and serving size validate."
        },
        "biometric_entry": {
            "type": "object",
            "additionalProperties": false,
            "required": ["type", "value"],
            "properties": {
                "type": {"const": "weight"},
                "value": {"type": "string", "pattern": "^(?:[1-9][0-9]*(?:\\.[0-9]+)?|0*\\.[0-9]*[1-9][0-9]*) kg$", "description": "Positive fixed-point weight in kilograms, for example 90 kg."}
            }
        }
    })
}

#[cfg(test)]
#[path = "../../unit-tests/profile_logs/mcp.rs"]
mod tests;
