use std::error::Error;

use axum::http::StatusCode;
use serde_json::json;

use crate::oauth::tests::{TEST_MCP_ACCESS_TOKEN, TEST_MCP_WRITE_ACCESS_TOKEN};
use crate::tests::{TestApplication, mcp_call_router_with_token};

#[test]
fn profile_tool_errors_preserve_structured_json_and_redact_sources() -> Result<(), Box<dyn Error>> {
    for case in crate::profiles::tests::profile_error_cases() {
        let result = super::profile_tool_error(case.error);
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
    app.seed_profile_owner().await?;
    crate::tests::mcp_initialize(&app).await?;

    let tools_response = app
        .request(crate::tests::protocol_request(json!({
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
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == name)
            .ok_or("missing profile tool")?;

        for schema_name in ["inputSchema", "outputSchema"] {
            let schema = &tool[schema_name];
            assert!(schema.is_object(), "{name} missing {schema_name}");
            crate::tests::assert_inline_schema(schema)?;
        }

        let output = &tool["outputSchema"];
        assert_eq!(output["type"], "object");
        assert_eq!(output["oneOf"][1]["required"], json!(["code", "error"]));
        assert_eq!(
            output["oneOf"][2]["properties"]["error"]["const"],
            "insufficient_scope"
        );
        let profile = if name == "list_profiles" {
            &output["oneOf"][0]["properties"]["items"]["items"]
        } else {
            &output["oneOf"][0]
        };
        let settings = &profile["properties"]["data"]["properties"]["settings"]["properties"];
        assert_eq!(settings["weight_change_goal"]["anyOf"][1]["type"], "null");
        for goal in [
            "daily_water_goal",
            "daily_step_goal",
            "daily_active_time_goal",
        ] {
            assert_eq!(settings[goal]["anyOf"][1]["type"], "null");
        }
        assert_eq!(
            settings["bmr"]["oneOf"][1]["properties"]["value"]["type"],
            "string"
        );
        assert_eq!(
            settings["baseline_activity"]["anyOf"][0]["oneOf"][0]["properties"]["level"]["enum"],
            json!([
                "no_activity",
                "sedentary",
                "lightly_active",
                "moderately_active",
                "very_active"
            ])
        );
    }
    assert!(!tools.iter().any(|tool| tool["name"] == "delete_profile"));

    let create_tool = tools
        .iter()
        .find(|tool| tool["name"] == "create_profile")
        .ok_or("create_profile schema is missing")?;
    assert_eq!(create_tool["inputSchema"]["required"], json!(["profile"]));
    let settings = &create_tool["inputSchema"]["properties"]["profile"]["properties"]["settings"]["anyOf"]
        [0]["properties"];
    assert_eq!(settings["weight_change_goal"]["anyOf"][0]["type"], "string");
    assert_eq!(
        settings["bmr"]["anyOf"][0]["oneOf"][1]["properties"]["value"]["type"],
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
    let patch = &update_tool["inputSchema"]["properties"]["patch"]["properties"];
    assert_eq!(
        patch["bmr"]["description"],
        "Null resets BMR to calculated mode."
    );
    assert_eq!(patch["weight_change_goal"]["anyOf"][0]["type"], "string");
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
    assert_schema_keys(
        &create_tool["outputSchema"]["oneOf"][2],
        &denied_create.json["result"]["structuredContent"],
    )?;
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
    let error = &invalid_birth_date.json["result"]["structuredContent"];
    assert_schema_keys(&create_tool["outputSchema"]["oneOf"][1], error)?;
    assert!(error["code"].is_string());
    assert!(error["error"].is_string());
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
                    "daily_water_goal": "3000 ml",
                    "daily_step_goal": 10000,
                    "daily_active_time_goal": "45 min",
                    "weight_change_goal": "-0.5 kg/week",
                    "bmr": {"mode":"custom", "value":"1650 kcal/day"},
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
    let profile_schema = &create_tool["outputSchema"]["oneOf"][0];
    let data_schema = &profile_schema["properties"]["data"];
    for (schema, value) in [
        (profile_schema, profile),
        (data_schema, &profile["data"]),
        (
            &data_schema["properties"]["settings"],
            &profile["data"]["settings"],
        ),
        (
            &data_schema["properties"]["cache"],
            &profile["data"]["cache"],
        ),
        (
            &profile_schema["properties"]["derived"],
            &profile["derived"],
        ),
    ] {
        assert_schema_keys(schema, value)?;
    }
    let profile_id = profile["id"].as_i64().ok_or("profile has no ID")?;
    assert_eq!(profile["data"]["settings"]["daily_water_goal"], "3000 ml");
    assert_eq!(profile["data"]["settings"]["daily_step_goal"], 10000);
    assert_eq!(
        profile["data"]["settings"]["daily_active_time_goal"],
        "2700 s"
    );
    assert_eq!(
        profile["data"]["settings"]["weight_change_goal"],
        "-0.5 kg/week"
    );
    crate::profiles::tests::assert_calorie_balance_goal(
        &profile["derived"]["calorie_balance_goal"],
        "-550 kcal/day",
    )?;
    assert_eq!(profile["data"]["settings"]["bmr"]["mode"], "custom");
    assert_eq!(profile["derived"]["bmr"], "1650 kcal/day");
    assert_eq!(profile["derived"]["baseline_activity"], "330.0 kcal/day");
    assert_eq!(
        profile["derived"]["maintenance_calories"],
        "1980.0 kcal/day"
    );

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
        json!({"id":profile_id, "patch":{"weight_goal":"64 kg", "daily_water_goal":null}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(updated.json["result"]["isError"], false);
    assert!(
        updated.json["result"]["structuredContent"]["data"]["settings"]["daily_water_goal"]
            .is_null()
    );
    assert_eq!(
        updated.json["result"]["structuredContent"]["data"]["settings"]["daily_step_goal"],
        10000
    );
    assert_eq!(
        updated.json["result"]["structuredContent"]["data"]["settings"]["weight_change_goal"],
        "-0.5 kg/week"
    );
    assert_eq!(
        updated.json["result"]["structuredContent"]["data"]["settings"]["weight_goal"],
        "64 kg"
    );

    let other_token = crate::profiles::tests::routes::seed_other_mcp_user_access(&app).await?;
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
    let list_tool = tools
        .iter()
        .find(|tool| tool["name"] == "list_profiles")
        .ok_or("list tool is missing")?;
    assert_schema_keys(
        &list_tool["outputSchema"]["oneOf"][0],
        &other_list.json["result"]["structuredContent"],
    )?;
    assert_eq!(
        other_list.json["result"]["structuredContent"]["items"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    Ok(())
}

#[tokio::test]
async fn mcp_weight_change_goal_handles_zero_clear_and_invalid_values() -> Result<(), Box<dyn Error>>
{
    let app = TestApplication::new().await?;
    app.seed_profile_owner().await?;
    crate::tests::mcp_initialize(&app).await?;
    let created = mcp_call_router_with_token(
        &app.router,
        1,
        "create_profile",
        json!({"profile":{"name":"Rate","settings":{"weight_change_goal":"-0.5 kg/week"}}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(
        created.json["result"]["isError"], false,
        "{:?}",
        created.json
    );
    let profile = &created.json["result"]["structuredContent"];
    assert!(profile["derived"]["maintenance_calories"].is_null());
    crate::profiles::tests::assert_calorie_balance_goal(
        &profile["derived"]["calorie_balance_goal"],
        "-550 kcal/day",
    )?;
    let id = profile["id"].as_i64().ok_or("profile has no ID")?;
    for invalid in ["1 kg/day"] {
        let response = mcp_call_router_with_token(
            &app.router,
            2,
            "update_profile",
            json!({"id":id,"patch":{"weight_change_goal":invalid}}),
            TEST_MCP_WRITE_ACCESS_TOKEN,
        )
        .await?;
        assert_eq!(response.json["result"]["isError"], true);
    }
    let gain = mcp_call_router_with_token(
        &app.router,
        9,
        "update_profile",
        json!({"id":id,"patch":{"weight_change_goal":"0.5 kg/week"}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(gain.json["result"]["isError"], false, "{:?}", gain.json);
    crate::profiles::tests::assert_calorie_balance_goal(
        &gain.json["result"]["structuredContent"]["derived"]["calorie_balance_goal"],
        "550 kcal/day",
    )?;
    let zero = mcp_call_router_with_token(
        &app.router,
        3,
        "update_profile",
        json!({"id":id,"patch":{"weight_change_goal":"0 kg/week"}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(zero.json["result"]["isError"], false, "{:?}", zero.json);
    crate::profiles::tests::assert_calorie_balance_goal(
        &zero.json["result"]["structuredContent"]["derived"]["calorie_balance_goal"],
        "0 kcal/day",
    )?;
    let clear = mcp_call_router_with_token(
        &app.router,
        4,
        "update_profile",
        json!({"id":id,"patch":{"weight_change_goal":null}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(clear.json["result"]["isError"], false);
    assert!(clear.json["result"]["structuredContent"]["derived"]["calorie_balance_goal"].is_null());
    Ok(())
}

fn assert_schema_keys(
    schema: &serde_json::Value,
    actual: &serde_json::Value,
) -> Result<(), Box<dyn Error>> {
    use std::collections::BTreeSet;
    let properties = schema["properties"]
        .as_object()
        .ok_or("schema properties missing")?;
    let required = schema["required"]
        .as_array()
        .ok_or("schema required missing")?;
    let object = actual.as_object().ok_or("response is not an object")?;
    let property_keys: BTreeSet<_> = properties.keys().map(String::as_str).collect();
    let actual_keys: BTreeSet<_> = object.keys().map(String::as_str).collect();
    let required_keys: BTreeSet<_> = required
        .iter()
        .map(|value| value.as_str().ok_or("invalid required key"))
        .collect::<Result<_, _>>()?;
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(property_keys, actual_keys);
    assert_eq!(required_keys, actual_keys);
    Ok(())
}
