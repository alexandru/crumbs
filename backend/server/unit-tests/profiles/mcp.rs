use std::error::Error;

use axum::http::StatusCode;
use serde_json::json;

use crate::oauth::tests::{TEST_MCP_ACCESS_TOKEN, TEST_MCP_WRITE_ACCESS_TOKEN};
use crate::profiles::tests::profile_error_cases;
use crate::tests::{TestApplication, mcp_call_router_with_token, mcp_initialize, protocol_request};

use crate::profiles::tests::routes::{seed_other_mcp_user_access, seed_profile_account};

use super::profile_tool_error;

#[test]
fn profile_tool_errors_preserve_structured_json_and_redact_sources() -> Result<(), Box<dyn Error>> {
    for case in profile_error_cases() {
        let result = profile_tool_error(case.error);
        let serialized = serde_json::to_value(result)?;

        assert_eq!(serialized["isError"], true);
        assert_eq!(
            serialized["structuredContent"],
            json!({"code": case.code, "error": case.message})
        );
        let serialized = serialized.to_string();
        assert!(!serialized.contains("private storage source detail"));
        assert!(!serialized.contains("private clock source detail"));
    }

    Ok(())
}

#[tokio::test]
async fn mcp_profile_tools_validate_schemas_scopes_and_per_call_account_identity()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    seed_profile_account(&app).await?;
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
        "create_profile",
        "list_profiles",
        "get_profile",
        "update_profile",
    ] {
        assert!(
            tools.iter().any(|tool| tool["name"] == name),
            "missing {name}"
        );
    }
    assert!(!tools.iter().any(|tool| tool["name"] == "delete_profile"));

    let create_tool = tools
        .iter()
        .find(|tool| tool["name"] == "create_profile")
        .ok_or("create_profile schema is missing")?;
    assert_eq!(create_tool["inputSchema"]["required"], json!(["profile"]));
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["create_profile"]["properties"]["settings"]["anyOf"][0]
            ["$ref"],
        "#/$defs/create_settings"
    );
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["bmr"]["oneOf"][1]["properties"]["value"]["type"],
        "string"
    );
    let update_tool = tools
        .iter()
        .find(|tool| tool["name"] == "update_profile")
        .ok_or("update_profile schema is missing")?;
    assert_eq!(
        update_tool["inputSchema"]["required"],
        json!(["id", "patch"])
    );
    assert_eq!(
        update_tool["inputSchema"]["$defs"]["patch_profile"]["properties"]["bmr"]["description"],
        "Null resets BMR to calculated mode."
    );
    assert_eq!(update_tool["annotations"]["destructiveHint"], true);

    let denied_create = mcp_call_router_with_token(
        &app.router,
        3,
        "create_profile",
        json!({"profile":{"name":"Read-only"}}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(denied_create.json["result"]["isError"], true);
    assert_eq!(
        denied_create.json["result"]["structuredContent"],
        json!({"error":"insufficient_scope", "required_scope":"crumbs:write"})
    );

    let invalid_birth_date = mcp_call_router_with_token(
        &app.router,
        10,
        "create_profile",
        json!({"profile":{"name":"Invalid date", "settings":{"birth_date":"1982-02-30"}}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(invalid_birth_date.json["result"]["isError"], true);
    assert_eq!(
        invalid_birth_date.json["result"]["structuredContent"]["code"],
        "invalid_input"
    );

    let created = mcp_call_router_with_token(
        &app.router,
        4,
        "create_profile",
        json!({
            "profile": {
                "name": "Amalia",
                "settings": {
                    "bmr": {"mode":"custom", "value":"1650 kcal"},
                    "baseline_activity": {"mode":"preset", "level":"sedentary"}
                }
            }
        }),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(created.status, StatusCode::OK, "{:?}", created.json);
    assert_eq!(created.json["result"]["isError"], false);
    let profile = &created.json["result"]["structuredContent"];
    let profile_id = profile["id"].as_i64().ok_or("profile has no ID")?;
    assert_eq!(profile["data"]["settings"]["bmr"]["mode"], "custom");
    assert_eq!(profile["derived"]["bmr"], "1650 kcal");
    assert_eq!(profile["derived"]["baseline_activity"], "330.0 kcal");
    assert_eq!(profile["derived"]["maintenance_calories"], "1980.0 kcal");

    let fetched = mcp_call_router_with_token(
        &app.router,
        5,
        "get_profile",
        json!({"id":profile_id}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(fetched.json["result"]["isError"], false);
    assert_eq!(fetched.json["result"]["structuredContent"], *profile);

    let updated = mcp_call_router_with_token(
        &app.router,
        6,
        "update_profile",
        json!({"id":profile_id, "patch":{"weight_goal":"64 kg"}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(updated.json["result"]["isError"], false);
    assert_eq!(
        updated.json["result"]["structuredContent"]["data"]["settings"]["weight_goal"],
        "64 kg"
    );

    let other_token = seed_other_mcp_user_access(&app).await?;
    let hidden = mcp_call_router_with_token(
        &app.router,
        7,
        "get_profile",
        json!({"id":profile_id}),
        &other_token,
    )
    .await?;
    assert_eq!(hidden.json["result"]["isError"], true);
    assert_eq!(
        hidden.json["result"]["structuredContent"],
        json!({"code":"not_found", "error":"profile not found"})
    );

    let hidden_update = mcp_call_router_with_token(
        &app.router,
        11,
        "update_profile",
        json!({"id":profile_id, "patch":{"name":"Cannot change Amalia"}}),
        &other_token,
    )
    .await?;
    assert_eq!(hidden_update.json["result"]["isError"], true);
    assert_eq!(
        hidden_update.json["result"]["structuredContent"]["code"],
        "not_found"
    );

    let same_name = mcp_call_router_with_token(
        &app.router,
        8,
        "create_profile",
        json!({"profile":{"name":"Amalia"}}),
        &other_token,
    )
    .await?;
    assert_eq!(same_name.json["result"]["isError"], false);
    let other_list =
        mcp_call_router_with_token(&app.router, 9, "list_profiles", json!({}), &other_token)
            .await?;
    assert_eq!(
        other_list.json["result"]["structuredContent"]["items"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    Ok(())
}
