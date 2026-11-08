use std::error::Error;

use axum::http::StatusCode;
use serde_json::json;

use crate::oauth::tests::{
    TEST_MCP_ACCESS_TOKEN, TEST_MCP_WRITE_ACCESS_TOKEN, TEST_REST_WRITE_ACCESS_TOKEN,
};
use crate::tests::{
    TestApplication, http_request, mcp_call_router_with_token, mcp_initialize, protocol_request,
};

use super::{create_test_profile, seed_other_access_token, seed_profile_owner};

const OTHER_MCP_TOKEN: &str = "test-other-profile-log-mcp-token";
const OTHER_MCP_REFRESH: &str = "test-other-profile-log-mcp-refresh";
const OTHER_MCP_CLIENT: &str = "test-other-profile-log-mcp-client";

#[tokio::test]
async fn mcp_profile_log_tools_cover_schema_scope_dispatch_and_account_isolation()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    seed_profile_owner(&app).await?;
    let (profile_id, _) = create_test_profile(&app, "Alex", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let profile_uri = format!("/profiles/{profile_id}");
    mcp_initialize(&app).await?;

    let tools_response = app
        .request(protocol_request(json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}
        }))?)
        .await?;
    assert_eq!(tools_response.status, StatusCode::OK);
    let tools = tools_response.json["result"]["tools"]
        .as_array()
        .ok_or("MCP tools list is missing")?;
    for name in [
        "create_profile_log",
        "list_profile_logs",
        "get_profile_log",
        "update_profile_log",
        "delete_profile_log",
    ] {
        assert!(
            tools.iter().any(|tool| tool["name"] == name),
            "missing {name}"
        );
    }

    let create_tool = tools
        .iter()
        .find(|tool| tool["name"] == "create_profile_log")
        .ok_or("create tool is missing")?;
    assert_eq!(
        create_tool["inputSchema"]["required"],
        json!(["profile_id", "log"])
    );
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["create_log"]["required"],
        json!(["day", "entry"])
    );
    assert!(
        create_tool["inputSchema"]["$defs"]["create_log"]["properties"]["time"]["anyOf"].is_array()
    );
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["create_log"]["additionalProperties"],
        false
    );
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["log_entry"]["properties"]["type"]["const"],
        "biometric"
    );
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["biometric_entry"]["properties"]["value"]["description"],
        "Positive fixed-point weight in kilograms, for example 90 kg."
    );
    let list_tool = tools
        .iter()
        .find(|tool| tool["name"] == "list_profile_logs")
        .ok_or("list tool is missing")?;
    assert_eq!(
        list_tool["inputSchema"]["properties"]["filter"]["anyOf"][0]["$ref"],
        "#/$defs/log_filter"
    );
    assert_eq!(
        list_tool["inputSchema"]["$defs"]["log_filter"]["properties"]["last_id"]["description"],
        "Exclusive cursor. Results are ordered by ID descending; next page uses id < last_id."
    );
    let log_filter = &list_tool["inputSchema"]["$defs"]["log_filter"];
    for field in ["from", "to", "entry_type", "subtype", "last_id"] {
        assert_eq!(
            log_filter["properties"][field]["anyOf"][1],
            json!({"type":"null"})
        );
    }
    assert_eq!(
        log_filter["allOf"][0]["if"]["properties"]["subtype"]["const"],
        "weight"
    );
    assert_eq!(
        log_filter["properties"]["entry_type"]["anyOf"][0]["enum"],
        json!(["food", "biometric"])
    );
    let update_tool = tools
        .iter()
        .find(|tool| tool["name"] == "update_profile_log")
        .ok_or("update tool is missing")?;
    assert_eq!(update_tool["annotations"]["destructiveHint"], true);
    let delete_tool = tools
        .iter()
        .find(|tool| tool["name"] == "delete_profile_log")
        .ok_or("delete tool is missing")?;
    assert_eq!(delete_tool["annotations"]["destructiveHint"], true);

    let create_args = json!({
        "profile_id": profile_id,
        "log": {
            "day": "2025-04-03",
            "time": "08:15",
            "entry": {
                "type": "biometric",
                "measurement": {"type": "weight", "value": "78.5 kg"}
            }
        }
    });
    let denied_create = mcp_call_router_with_token(
        &app.router,
        3,
        "create_profile_log",
        create_args.clone(),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(denied_create.json["result"]["isError"], true);
    assert_eq!(
        denied_create.json["result"]["structuredContent"],
        json!({"error":"insufficient_scope", "required_scope":"crumbs:write"})
    );

    let invalid_weight = mcp_call_router_with_token(
        &app.router,
        14,
        "create_profile_log",
        json!({
            "profile_id":profile_id,
            "log":{"day":"2025-04-03", "entry":{"type":"biometric", "measurement":{"type":"weight", "value":"0 kg"}}}
        }),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(invalid_weight.json["result"]["isError"], true);
    assert_eq!(
        invalid_weight.json["result"]["structuredContent"]["code"],
        "invalid_input"
    );

    let created = mcp_call_router_with_token(
        &app.router,
        4,
        "create_profile_log",
        create_args,
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(created.status, StatusCode::OK, "{:?}", created.json);
    assert_eq!(created.json["result"]["isError"], false);
    let log = &created.json["result"]["structuredContent"];
    let log_id = log["id"].as_i64().ok_or("log has no ID")?;
    assert_eq!(log["time"], "08:15:00.000");
    assert_eq!(
        log["data"],
        json!({"type":"biometric", "measurement":{"type":"weight", "value":"78.5 kg"}})
    );

    let get = mcp_call_router_with_token(
        &app.router,
        5,
        "get_profile_log",
        json!({"profile_id":profile_id, "id":log_id}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(get.json["result"]["isError"], false);
    assert_eq!(get.json["result"]["structuredContent"], *log);

    let list = mcp_call_router_with_token(
        &app.router,
        6,
        "list_profile_logs",
        json!({"profile_id":profile_id, "filter":{"from":"2025-04-03", "to":"2025-04-03", "entry_type":"biometric", "subtype":"weight"}}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(list.json["result"]["isError"], false);
    assert_eq!(
        list.json["result"]["structuredContent"]["items"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        list.json["result"]["structuredContent"]["items"][0]["id"],
        log_id
    );

    let null_filter = mcp_call_router_with_token(
        &app.router,
        15,
        "list_profile_logs",
        json!({
            "profile_id": profile_id,
            "filter": {
                "from": null,
                "to": null,
                "entry_type": null,
                "subtype": null,
                "last_id": null
            }
        }),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(null_filter.json["result"]["isError"], false);
    assert_eq!(
        null_filter.json["result"]["structuredContent"]["items"][0]["id"],
        log_id
    );

    let denied_update = mcp_call_router_with_token(
        &app.router,
        7,
        "update_profile_log",
        json!({"profile_id":profile_id, "id":log_id, "patch":{"day":"2025-04-04"}}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(denied_update.json["result"]["isError"], true);
    assert_eq!(
        denied_update.json["result"]["structuredContent"]["required_scope"],
        "crumbs:write"
    );

    let updated = mcp_call_router_with_token(
        &app.router,
        8,
        "update_profile_log",
        json!({
            "profile_id":profile_id,
            "id":log_id,
            "patch":{"entry":{"type":"biometric", "measurement":{"type":"weight", "value":"79 kg"}}}
        }),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(updated.json["result"]["isError"], false);
    assert_eq!(
        updated.json["result"]["structuredContent"]["data"]["measurement"]["value"],
        "79 kg"
    );
    let profile_after_update = app.request(http_request(&profile_uri)?).await?;
    assert_eq!(
        profile_after_update.json["data"]["cache"]["current_weight"],
        "79 kg"
    );
    assert!(profile_after_update.json["derived"]["bmr"].is_string());

    let other_token = seed_other_access_token(
        &app,
        true,
        OTHER_MCP_TOKEN,
        OTHER_MCP_REFRESH,
        OTHER_MCP_CLIENT,
    )
    .await?;
    for (id, tool, args) in [
        (
            9,
            "get_profile_log",
            json!({"profile_id":profile_id, "id":log_id}),
        ),
        (
            10,
            "list_profile_logs",
            json!({"profile_id":profile_id, "filter":{}}),
        ),
        (
            11,
            "update_profile_log",
            json!({"profile_id":profile_id, "id":log_id, "patch":{"day":"2025-04-05"}}),
        ),
        (
            12,
            "delete_profile_log",
            json!({"profile_id":profile_id, "id":log_id}),
        ),
    ] {
        let hidden = mcp_call_router_with_token(&app.router, id, tool, args, &other_token).await?;
        assert_eq!(hidden.json["result"]["isError"], true);
        assert_eq!(
            hidden.json["result"]["structuredContent"]["code"],
            "not_found"
        );
    }

    let deleted = mcp_call_router_with_token(
        &app.router,
        13,
        "delete_profile_log",
        json!({"profile_id":profile_id, "id":log_id}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(deleted.json["result"]["isError"], false);
    assert_eq!(
        deleted.json["result"]["structuredContent"],
        json!({"id":log_id, "deleted":true})
    );
    let profile_after_delete = app.request(http_request(&profile_uri)?).await?;
    assert_eq!(
        profile_after_delete.json["data"]["cache"]["current_weight"],
        serde_json::Value::Null
    );
    assert_eq!(
        profile_after_delete.json["derived"]["bmr"],
        serde_json::Value::Null
    );
    Ok(())
}
