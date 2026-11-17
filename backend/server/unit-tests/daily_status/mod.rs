use std::error::Error;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use crumbs_database::identity::{OAuthScope, PasswordHash, Username, Users};
use serde_json::json;

use crate::oauth::tests::{TEST_MCP_RESOURCE, TEST_REST_RESOURCE, TEST_REST_WRITE_ACCESS_TOKEN};
use crate::tests::{
    TestApplication, http_json_request, http_request, http_request_with_token, mcp_call,
    mcp_call_router_with_token, mcp_initialize, protocol_request,
};

#[tokio::test]
async fn daily_status_rest_and_mcp_cover_dates_scope_and_ownership() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    app.seed_profile_owner().await?;
    let created = app
        .request(http_json_request(
            "POST",
            "/profiles",
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"name": "Daily status", "settings": {"timezone": "UTC", "daily_step_goal": 10000, "daily_active_time_goal": "60 min"}}),
        )?)
        .await?;
    assert_eq!(created.status, StatusCode::CREATED);
    let profile = created.json["id"].as_i64().ok_or("profile id is missing")?;
    let logs = app.request(http_json_request(
        "POST", &format!("/profiles/{profile}/logs/batch"), TEST_REST_WRITE_ACCESS_TOKEN,
        json!({"logs": [
            {"day":"2025-04-03", "time":"08:00", "entry":{"type":"exercise", "exercise_type":"walking", "intensity":"moderate", "duration":"30 min", "calories_burned":"150 kcal"}},
            {"day":"2025-04-03", "time":"12:00", "entry":{"type":"daily_activity", "reading":{"type":"steps", "value":9000}}},
            {"day":"2025-04-03", "time":"16:00", "entry":{"type":"daily_activity", "reading":{"type":"steps", "value":8000}}},
            {"day":"2025-04-03", "time":"16:00", "entry":{"type":"daily_activity", "reading":{"type":"active_time", "value":"30 min"}}}
        ]}),
    )?).await?;
    assert_eq!(logs.status, StatusCode::CREATED, "{:?}", logs.json);
    mcp_initialize(&app).await?;

    let tools = app
        .request(protocol_request(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}
        }))?)
        .await?;
    let tool = tools.json["result"]["tools"]
        .as_array()
        .ok_or("tools missing")?
        .iter()
        .find(|tool| tool["name"] == "get_daily_status")
        .ok_or("tool missing")?;
    assert_eq!(tool["inputSchema"]["required"], json!(["profile_id"]));
    assert_eq!(tool["annotations"]["readOnlyHint"], true);

    let path = format!("/profiles/{profile}/daily-status");
    let unauthenticated = app
        .request(Request::builder().uri(&path).body(Body::empty())?)
        .await?;
    assert_eq!(unauthenticated.status, StatusCode::UNAUTHORIZED);
    let invalid_date = app
        .request(http_request(&format!("{path}?start=2025-02-30"))?)
        .await?;
    assert_eq!(invalid_date.status, StatusCode::BAD_REQUEST);
    let rest = app
        .request(http_request(&format!(
            "{path}?start=2025-04-03&stop=2025-04-04"
        ))?)
        .await?;
    assert_eq!(rest.status, StatusCode::OK);
    assert_eq!(rest.json["start"], "2025-04-03");
    assert_eq!(rest.json["stop"], "2025-04-04");
    assert_eq!(rest.json["days"].as_array().map(Vec::len), Some(2));
    assert_eq!(rest.json["days"][0]["steps"], 8000);
    assert_eq!(rest.json["days"][0]["active_time"], "1800 s");
    assert_eq!(rest.json["days"][0]["expenditure"]["exercise"], "150 kcal");
    assert_eq!(
        rest.json["days"][0]["goals"]["steps"],
        json!({"target":10000, "percent":"80"})
    );
    assert_eq!(
        rest.json["days"][0]["goals"]["active_time"],
        json!({"target":"3600 s", "percent":"50"})
    );
    let mcp = mcp_call(
        &app,
        2,
        "get_daily_status",
        json!({
            "profile_id": profile, "start": "2025-04-03", "stop": "2025-04-04"
        }),
    )
    .await?;
    assert_eq!(mcp.json["result"]["isError"], false);
    assert_eq!(mcp.json["result"]["structuredContent"], rest.json);

    let one = app
        .request(http_request(&format!("{path}?stop=2025-04-03"))?)
        .await?;
    assert_eq!(one.status, StatusCode::OK);
    assert_eq!(one.json["start"], "2025-04-03");
    assert_eq!(one.json["stop"], "2025-04-03");
    let today = app.request(http_request(&path)?).await?;
    assert_eq!(today.status, StatusCode::OK);
    assert_eq!(today.json["start"], today.json["stop"]);
    assert_eq!(today.json["days"].as_array().map(Vec::len), Some(1));
    let invalid = app
        .request(http_request(&format!(
            "{path}?start=2025-04-04&stop=2025-04-03"
        ))?)
        .await?;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    let invalid_tool = mcp_call(
        &app,
        3,
        "get_daily_status",
        json!({
            "profile_id": profile, "start": "2025-04-04", "stop": "2025-04-03"
        }),
    )
    .await?;
    assert_eq!(invalid_tool.json["result"]["isError"], true);
    assert_eq!(
        invalid_tool.json["result"]["structuredContent"]["code"],
        "invalid_input"
    );

    let auth = app._authorization_database.as_ref();
    let username = Username::new("daily-status-other")?;
    let password = PasswordHash::hash("daily status other password")?;
    let content_user = app.database.create_user(&username, &password).await?;
    let user = auth.create_user(&username, &password).await?;
    assert_eq!(content_user.id, user.id);
    for (resource, token, refresh, client) in [
        (
            TEST_REST_RESOURCE,
            "daily-other-rest",
            "daily-other-rest-refresh",
            "daily-other-rest-client",
        ),
        (
            TEST_MCP_RESOURCE,
            "daily-other-mcp",
            "daily-other-mcp-refresh",
            "daily-other-mcp-client",
        ),
    ] {
        crate::oauth::tests::seed_session_for_user(
            auth,
            client,
            resource,
            token,
            refresh,
            OAuthScope::Read,
            (user.id, password.clone()),
        )
        .await?;
    }
    let hidden = app
        .request(http_request_with_token(&path, "daily-other-rest")?)
        .await?;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    let hidden_tool = mcp_call_router_with_token(
        &app.router,
        4,
        "get_daily_status",
        json!({"profile_id": profile}),
        "daily-other-mcp",
    )
    .await?;
    assert_eq!(
        hidden_tool.json["result"]["structuredContent"]["code"],
        "not_found"
    );
    Ok(())
}
