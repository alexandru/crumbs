use std::sync::Arc;

use axum::http::request::Parts;
use crumbs_database::daily_status::{DailyStatusQuery, DailyStatusStore};
use crumbs_database::identity::{OAuthScope, UserSession};
use crumbs_database::profile_logs::LogDay;
use crumbs_database::profiles::ProfileId;
use rmcp::ErrorData;
use rmcp::handler::server::router::tool::ToolRoute;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolResult, Tool, ToolAnnotations};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::mcp::ToolRegistration;

pub(crate) fn mcp_tools<S>() -> impl Iterator<Item = ToolRegistration<S>>
where
    S: DailyStatusStore + Send + Sync + 'static,
{
    let route: ToolRoute<Arc<S>> =
        ToolRoute::new_dyn(tool(), |context: ToolCallContext<'_, Arc<S>>| {
            Box::pin(async move {
                let user = context
                    .request_context()
                    .extensions
                    .get::<Parts>()
                    .and_then(|parts| parts.extensions.get::<UserSession>())
                    .map(|session| session.user_id);
                let Some(user) = user else {
                    return Ok(tool_error(
                        "unauthorized",
                        "authenticated user session is required",
                    )
                    .into());
                };
                let input = match serde_json::from_value::<Request>(Value::Object(
                    context.arguments.unwrap_or_default(),
                )) {
                    Ok(input) => input,
                    Err(error) => return Ok(tool_error("invalid_input", error.to_string()).into()),
                };
                match context
                    .service
                    .daily_status(
                        user,
                        input.profile_id,
                        &DailyStatusQuery {
                            start: input.start,
                            stop: input.stop,
                        },
                    )
                    .await
                {
                    Ok(Some(report)) => {
                        let value = serde_json::to_value(report).map_err(|_| {
                            ErrorData::internal_error("daily status serialization failed", None)
                        })?;
                        Ok(CallToolResult::structured(value).into())
                    }
                    Ok(None) => Ok(tool_error("not_found", "profile not found").into()),
                    Err(error) => {
                        let (_, axum::Json(value)) = super::handlers::status_error(error);
                        Ok(CallToolResult::structured_error(value).into())
                    }
                }
            })
        });
    std::iter::once(ToolRegistration::new(route, OAuthScope::Read))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    profile_id: ProfileId,
    start: Option<LogDay>,
    stop: Option<LogDay>,
}

fn tool() -> Tool {
    let schema = Map::from_iter([
        ("type".to_owned(), json!("object")),
        ("additionalProperties".to_owned(), json!(false)),
        ("required".to_owned(), json!(["profile_id"])),
        (
            "properties".to_owned(),
            json!({
                "profile_id": {"type": "integer", "minimum": 1},
                "start": {"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"},
                "stop": {"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"}
            }),
        ),
    ]);
    Tool::new_with_raw("get_daily_status", Some("Get daily status for an account-owned profile. Dates are inclusive. Omit both dates for today in the profile timezone; one date selects that day. Current profile settings and maintenance apply to historical days. Nutrient totals sum recorded reference amounts; complete and exact flags describe the recorded data. Water uses an estimate of 1 g per ml. Missing activity readings are null. The calorie target is maintenance plus the configured balance, without adding exercise. Percent is null for zero or negative targets and may exceed 100. Requires crumbs:read.".into()), schema)
        .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).open_world(false))
}

fn tool_error(code: &'static str, message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(json!({"code": code, "error": message.into()}))
}
