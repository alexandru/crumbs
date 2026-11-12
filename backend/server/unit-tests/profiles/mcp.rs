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
    crate::profiles::tests::routes::seed_profile_account(&app).await?;
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
            assert_local_references(schema, schema)?;
        }
        let output = &tool["outputSchema"];
        assert_eq!(output["type"], "object");
        assert_eq!(output["oneOf"][1]["$ref"], "#/$defs/error");
        assert_eq!(output["oneOf"][2]["$ref"], "#/$defs/scope_error");
        let defs = &output["$defs"];
        if name == "list_profiles" {
            assert_eq!(output["oneOf"][0]["$ref"], "#/$defs/profile_list");
            assert_eq!(
                defs["profile_list"]["properties"]["items"]["items"]["$ref"],
                "#/$defs/profile"
            );
        } else {
            assert_eq!(output["oneOf"][0]["$ref"], "#/$defs/profile");
        }
        assert_eq!(
            defs["profile_settings"]["properties"]["bmr"]["$ref"],
            "#/$defs/bmr"
        );
        assert_eq!(
            defs["profile_settings"]["properties"]["weight_change_goal"]["anyOf"][1]["type"],
            "null"
        );
        assert_eq!(defs["daily_energy"]["type"], json!(["string", "null"]));
    }
    assert!(!tools.iter().any(|tool| tool["name"] == "delete_profile"));

    let create_tool = tools
        .iter()
        .find(|tool| tool["name"] == "create_profile")
        .ok_or("create_profile schema is missing")?;
    assert_eq!(create_tool["inputSchema"]["required"], json!(["profile"]));
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["create_settings"]["properties"]["weight_change_goal"]
            ["anyOf"][0]["$ref"],
        "#/$defs/weight_change_goal"
    );
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["weight_change_goal"]["type"],
        "string"
    );
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
    assert_eq!(
        update_tool["inputSchema"]["$defs"]["patch_profile"]["properties"]["weight_change_goal"]["anyOf"]
            [0]["$ref"],
        "#/$defs/weight_change_goal"
    );
    assert_eq!(
        update_tool["inputSchema"]["$defs"]["weight_change_goal"]["type"],
        "string"
    );

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
        &create_tool["outputSchema"]["$defs"]["scope_error"],
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
    assert_schema_keys(&create_tool["outputSchema"]["$defs"]["error"], error)?;
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
    let defs = &create_tool["outputSchema"]["$defs"];
    for (name, value) in [
        ("profile", profile),
        ("profile_data", &profile["data"]),
        ("profile_settings", &profile["data"]["settings"]),
        ("profile_cache", &profile["data"]["cache"]),
        ("profile_derived", &profile["derived"]),
    ] {
        assert_schema_keys(&defs[name], value)?;
    }
    let profile_id = profile["id"].as_i64().ok_or("profile has no ID")?;
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
        json!({"id":profile_id, "patch":{"weight_goal":"64 kg"}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(updated.json["result"]["isError"], false);
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
        &list_tool["outputSchema"]["$defs"]["profile_list"],
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

fn assert_local_references(
    root: &serde_json::Value,
    value: &serde_json::Value,
) -> Result<(), Box<dyn Error>> {
    match value {
        serde_json::Value::Object(fields) => {
            if let Some(reference) = fields.get("$ref") {
                let reference = reference.as_str().ok_or("non-string schema reference")?;
                let pointer = reference
                    .strip_prefix('#')
                    .ok_or("non-local schema reference")?;
                if root.pointer(pointer).is_none() {
                    return Err(format!("unresolved schema reference: {reference}").into());
                }
            }
            for child in fields.values() {
                assert_local_references(root, child)?;
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                assert_local_references(root, child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[tokio::test]
async fn mcp_weight_change_goal_handles_zero_clear_and_invalid_values() -> Result<(), Box<dyn Error>>
{
    let app = TestApplication::new().await?;
    crate::profiles::tests::routes::seed_profile_account(&app).await?;
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
