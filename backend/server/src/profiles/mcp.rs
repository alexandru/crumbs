//! MCP schemas and invocation for account-owned profiles.

use std::sync::Arc;

use axum::http::request::Parts;
use crumbs_database::identity::{OAuthScope, UserId, UserSession};
use crumbs_database::profiles::{
    CreateProfile, PatchProfile, ProfileError, ProfileId, ProfileListQuery, Profiles,
};
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
    S: Profiles + Send + Sync + 'static,
{
    [
        ProfileTool::List,
        ProfileTool::Get,
        ProfileTool::Create,
        ProfileTool::Update,
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
enum ProfileTool {
    List,
    Get,
    Create,
    Update,
}

impl ProfileTool {
    const fn requires_write(self) -> bool {
        matches!(self, Self::Create | Self::Update)
    }

    fn tool(self) -> Tool {
        let description = match self {
            Self::List => {
                "List profiles owned by the authenticated account in ascending profile ID order. Use last_id as an exclusive cursor. Requires crumbs:read."
            }
            Self::Get => {
                "Get one profile owned by the authenticated account. Requires crumbs:read."
            }
            Self::Create => {
                "Create a named profile for the authenticated account. Omitted settings use profile defaults. Requires crumbs:write."
            }
            Self::Update => {
                "Patch supported settings for a profile owned by the authenticated account. Omitted fields remain unchanged; null clears nullable settings and resets BMR to calculated. Requires crumbs:write."
            }
        };

        Tool::new_with_raw(self.name(), Some(description.into()), self.schema()).with_annotations(
            ToolAnnotations::new()
                .read_only(!self.requires_write())
                .destructive(matches!(self, Self::Update))
                .open_world(false),
        )
    }

    const fn name(self) -> &'static str {
        match self {
            Self::List => "list_profiles",
            Self::Get => "get_profile",
            Self::Create => "create_profile",
            Self::Update => "update_profile",
        }
    }

    fn schema(self) -> Map<String, Value> {
        let mut schema = Map::from_iter([
            ("type".to_owned(), json!("object")),
            ("additionalProperties".to_owned(), json!(false)),
        ]);
        let id = json!({"type": "integer", "minimum": 1, "description": "Positive profile ID."});

        match self {
            Self::List => {
                schema.insert(
                    "properties".to_owned(),
                    json!({
                        "last_id": {"type": ["integer", "null"], "minimum": 1, "description": "Exclusive profile ID cursor. Results are ordered by ID ascending."},
                        "limit": {"type": ["integer", "null"], "minimum": 1, "maximum": 100, "default": 100}
                    }),
                );
            }
            Self::Get => {
                schema.insert("required".to_owned(), json!(["id"]));
                schema.insert("properties".to_owned(), json!({"id": id}));
            }
            Self::Create => {
                schema.insert("required".to_owned(), json!(["profile"]));
                schema.insert(
                    "properties".to_owned(),
                    json!({"profile": {"$ref": "#/$defs/create_profile"}}),
                );
                schema.insert("$defs".to_owned(), definitions());
            }
            Self::Update => {
                schema.insert("required".to_owned(), json!(["id", "patch"]));
                schema.insert(
                    "properties".to_owned(),
                    json!({
                        "id": id,
                        "patch": {"$ref": "#/$defs/patch_profile"}
                    }),
                );
                schema.insert("$defs".to_owned(), definitions());
            }
        }

        schema
    }

    async fn call<S: Profiles + ?Sized>(
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
            Err(ToolFailure::Profile(error)) => Ok(profile_tool_error(error).into()),
            Err(ToolFailure::NotFound) => Ok(tool_error("not_found", "profile not found").into()),
            Err(ToolFailure::InvalidInput(message)) => {
                Ok(tool_error("invalid_input", message).into())
            }
            Err(ToolFailure::Serialization) => Err(ErrorData::internal_error(
                "profile serialization failed",
                None,
            )),
        }
    }

    async fn execute<S: Profiles + ?Sized>(
        self,
        store: &S,
        user: UserId,
        arguments: Value,
    ) -> Result<Value, ToolFailure> {
        match self {
            Self::List => {
                let query: ProfileListQuery = parse(arguments)?;

                let profiles = store
                    .list_profiles(user, &query)
                    .await
                    .map_err(ToolFailure::Profile)?;

                serialize(profiles)
            }
            Self::Get => {
                let request: IdRequest = parse(arguments)?;

                let profile = store
                    .profile(user, request.id)
                    .await
                    .map_err(ToolFailure::Profile)?
                    .ok_or(ToolFailure::NotFound)?;

                serialize(profile)
            }
            Self::Create => {
                let request: CreateRequest = parse(arguments)?;

                let profile = store
                    .create_profile(user, &request.profile)
                    .await
                    .map_err(ToolFailure::Profile)?;

                serialize(profile)
            }
            Self::Update => {
                let request: UpdateRequest = parse(arguments)?;

                let profile = store
                    .patch_profile(user, request.id, &request.patch)
                    .await
                    .map_err(ToolFailure::Profile)?
                    .ok_or(ToolFailure::NotFound)?;

                serialize(profile)
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdRequest {
    id: ProfileId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    profile: CreateProfile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    id: ProfileId,
    patch: PatchProfile,
}

enum ToolFailure {
    InvalidInput(String),
    NotFound,
    Profile(ProfileError),
    Serialization,
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, ToolFailure> {
    serde_json::from_value(value).map_err(|error| ToolFailure::InvalidInput(error.to_string()))
}

fn serialize<T: Serialize>(value: T) -> Result<Value, ToolFailure> {
    serde_json::to_value(value).map_err(|_| ToolFailure::Serialization)
}

fn profile_tool_error(error: ProfileError) -> CallToolResult {
    let code = error.code();
    let message = match error {
        ProfileError::InvalidInput(message) | ProfileError::Calculation(message) => message,
        ProfileError::DuplicateName => "profile name already exists for this account".to_owned(),
        ProfileError::InvalidStoredData | ProfileError::Storage(_) | ProfileError::Clock(_) => {
            "profile operation failed".to_owned()
        }
    };

    tool_error(code, message)
}

fn tool_error(code: &'static str, message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(json!({"code": code, "error": message.into()}))
}

fn definitions() -> Value {
    json!({
        "create_profile": {
            "type": "object",
            "additionalProperties": false,
            "required": ["name"],
            "properties": {
                "name": {"type": "string", "minLength": 1, "pattern": "\\S", "description": "Profile display name. Surrounding whitespace is trimmed; names are unique per account ignoring case, while accents remain significant."},
                "settings": {"anyOf": [{"$ref": "#/$defs/create_settings"}, {"type": "null"}]}
            }
        },
        "create_settings": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "birth_date": {"anyOf": [{"$ref": "#/$defs/birth_date"}, {"type": "null"}]},
                "sex": {"anyOf": [{"$ref": "#/$defs/sex"}, {"type": "null"}]},
                "height": {"anyOf": [{"$ref": "#/$defs/height"}, {"type": "null"}]},
                "timezone": {"type": ["string", "null"], "description": "IANA timezone name. Defaults to UTC."},
                "weight_goal": {"anyOf": [{"$ref": "#/$defs/weight"}, {"type": "null"}]},
                "bmr": {"anyOf": [{"$ref": "#/$defs/bmr"}, {"type": "null"}]},
                "baseline_activity": {"anyOf": [{"$ref": "#/$defs/baseline_activity"}, {"type": "null"}]}
            }
        },
        "patch_profile": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "name": {"type": "string", "minLength": 1, "pattern": "\\S"},
                "birth_date": {"anyOf": [{"$ref": "#/$defs/birth_date"}, {"type": "null"}]},
                "sex": {"anyOf": [{"$ref": "#/$defs/sex"}, {"type": "null"}]},
                "height": {"anyOf": [{"$ref": "#/$defs/height"}, {"type": "null"}]},
                "timezone": {"type": "string", "description": "IANA timezone name."},
                "weight_goal": {"anyOf": [{"$ref": "#/$defs/weight"}, {"type": "null"}]},
                "bmr": {"anyOf": [{"$ref": "#/$defs/bmr"}, {"type": "null"}], "description": "Null resets BMR to calculated mode."},
                "baseline_activity": {"anyOf": [{"$ref": "#/$defs/baseline_activity"}, {"type": "null"}]}
            }
        },
        "birth_date": {"type": "string", "pattern": "^[0-9]{4}(-[0-9]{2}(-[0-9]{2})?)?$", "description": "Valid calendar date in YYYY, YYYY-MM, or YYYY-MM-DD form."},
        "sex": {"type": "string", "enum": ["male", "female"]},
        "height": {"type": "string", "description": "Positive height in centimetres, for example 175 cm."},
        "weight": {"type": "string", "description": "Positive body weight with the kg unit, for example 70 kg."},
        "bmr": {
            "oneOf": [
                {"type": "object", "additionalProperties": false, "required": ["mode"], "properties": {"mode": {"const": "calculated"}}},
                {"type": "object", "additionalProperties": false, "required": ["mode", "value"], "properties": {"mode": {"const": "custom"}, "value": {"type": "string", "description": "Positive BMR in kcal/day, serialized with the kcal unit, for example 1700 kcal."}}}
            ]
        },
        "baseline_activity": {
            "oneOf": [
                {"type": "object", "additionalProperties": false, "required": ["mode", "level"], "properties": {"mode": {"const": "preset"}, "level": {"type": "string", "enum": ["no_activity", "sedentary", "lightly_active", "moderately_active", "very_active"]}}},
                {"type": "object", "additionalProperties": false, "required": ["mode", "value"], "properties": {"mode": {"const": "custom"}, "value": {"type": "string", "description": "Nonnegative fixed baseline activity in kcal/day, serialized with the kcal unit, for example 400 kcal."}}}
            ]
        }
    })
}

#[cfg(test)]
#[path = "../../unit-tests/profiles/mcp.rs"]
mod tests;
