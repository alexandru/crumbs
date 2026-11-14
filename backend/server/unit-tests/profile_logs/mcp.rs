use std::error::Error;

use axum::http::StatusCode;
use serde_json::json;

use crate::oauth::tests::{
    TEST_MCP_ACCESS_TOKEN, TEST_MCP_WRITE_ACCESS_TOKEN, TEST_REST_WRITE_ACCESS_TOKEN,
};
use crate::profile_logs::tests::{
    assert_calories, assert_mass, create_test_food, create_test_profile, profile_log_error_cases,
    seed_other_access_token,
};
use crate::tests::{
    TestApplication, http_json_request, http_request, mcp_call_router_with_token, mcp_initialize,
    protocol_request,
};

use super::profile_log_tool_error;

#[tokio::test]
async fn mcp_profile_log_error_adapter_preserves_codes_and_redacts_internal_details() {
    for case in profile_log_error_cases() {
        let result = profile_log_tool_error(case.error);

        assert_eq!(result.is_error, Some(true), "{}", case.name);
        assert_eq!(
            result.structured_content,
            Some(json!({"code": case.code, "error": case.message})),
            "{}",
            case.name
        );
    }
}

const OTHER_MCP_TOKEN: &str = "test-other-profile-log-mcp-token";
const OTHER_MCP_REFRESH: &str = "test-other-profile-log-mcp-refresh";
const OTHER_MCP_CLIENT: &str = "test-other-profile-log-mcp-client";

#[tokio::test]
async fn mcp_profile_log_tools_cover_schema_scope_dispatch_and_account_isolation()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    app.seed_profile_owner().await?;
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
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == name)
            .ok_or_else(|| format!("missing {name}"))?;

        let schema = &tool["inputSchema"];
        assert!(schema.is_object(), "missing inputSchema for {name}");
        crate::tests::assert_local_references(schema, schema)?;
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
        create_tool["inputSchema"]["$defs"]["log_entry"]["oneOf"][0]["$ref"],
        "#/$defs/biometric_log_entry"
    );
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["log_entry"]["oneOf"][1]["$ref"],
        "#/$defs/food_log_entry"
    );
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["food_log_entry"]["required"],
        json!(["type", "food_id", "amount", "serving_size"])
    );
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["food_log_entry"]["additionalProperties"],
        false
    );
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["food_log_entry"]["properties"]["type"]["const"],
        "food"
    );
    assert_eq!(
        create_tool["inputSchema"]["$defs"]["food_log_entry"]["properties"]["food_id"]["minimum"],
        1
    );
    assert!(create_tool["inputSchema"]["$defs"]["food_log_entry"]["properties"]["serving_size"]["description"]
        .as_str()
        .is_some_and(|description| description.contains("µg") && description.contains("case-sensitive")));
    assert!(
        create_tool["description"].as_str().is_some_and(
            |description| description.contains("edit the food or create a food with calories")
        )
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
        json!(["food", "biometric", "journal", "exercise"])
    );
    let update_tool = tools
        .iter()
        .find(|tool| tool["name"] == "update_profile_log")
        .ok_or("update tool is missing")?;
    assert_eq!(update_tool["annotations"]["destructiveHint"], true);
    assert_eq!(
        update_tool["inputSchema"]["$defs"]["log_entry_patch"]["oneOf"][1]["$ref"],
        "#/$defs/food_log_entry_patch"
    );
    assert_eq!(
        update_tool["inputSchema"]["$defs"]["food_log_entry_patch"]["required"],
        json!(["type"])
    );
    assert_eq!(
        update_tool["inputSchema"]["$defs"]["food_log_entry_patch"]["additionalProperties"],
        false
    );
    assert!(
        update_tool["inputSchema"]["$defs"]["food_log_entry_patch"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("preserves the existing snapshot"))
    );
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

#[tokio::test]
async fn mcp_profile_logs_create_and_patch_food_snapshots() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    app.seed_profile_owner().await?;
    let (profile_id, _) = create_test_profile(&app, "Amalia", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    let profile_uri = format!("/profiles/{profile_id}");
    let logs_uri = format!("{profile_uri}/logs");
    let food_id = create_test_food(
        &app,
        "Berry drink",
        json!({
            "nutrition": {"per":"100 ml", "energy":"50 kcal", "protein":"2 g"},
            "serving_sizes": [{"name":"one cup", "quantity":"250 ml"}]
        }),
    )
    .await?;
    let mass_food_id = create_test_food(
        &app,
        "Peanut bar",
        json!({"nutrition":{"per":"100 g", "energy":"200 kcal", "protein":"10 g"}}),
    )
    .await?;
    let no_calories_id = create_test_food(
        &app,
        "Drink without calories",
        json!({"nutrition":{"per":"100 ml", "protein":"1 g"}}),
    )
    .await?;
    let weight = app
        .request(http_json_request(
            "POST",
            &logs_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({
                "day":"2025-09-01",
                "time":"08:00",
                "entry":{"type":"biometric", "measurement":{"type":"weight", "value":"65 kg"}}
            }),
        )?)
        .await?;
    assert_eq!(weight.status, StatusCode::CREATED, "{:?}", weight.json);

    mcp_initialize(&app).await?;
    let created = mcp_call_router_with_token(
        &app.router,
        21,
        "create_profile_log",
        json!({
            "profile_id":profile_id,
            "log": {
                "day":"2025-09-02",
                "entry":{"type":"food", "food_id":food_id, "amount":"2", "serving_size":"one cup"}
            }
        }),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(created.json["result"]["isError"], false);
    let log = &created.json["result"]["structuredContent"];
    let log_id = log["id"].as_i64().ok_or("food log ID is missing")?;
    assert_eq!(log["data"]["type"], "food");
    assert_eq!(log["data"]["food_id"], food_id);
    assert_eq!(log["data"]["quantity"], "500 ml");
    assert_calories(&log["data"]["nutrition"]["calories"], "250 kcal")?;
    assert_mass(&log["data"]["nutrition"]["protein"], "10 g")?;
    assert_eq!(log["data"]["description"], "Berry drink");
    assert_eq!(log["data"]["input"]["amount"], "2");
    assert_eq!(log["data"]["input"]["serving_size"]["name"], "one cup");
    assert_eq!(log["data"]["input"]["serving_size"]["quantity"], "250 ml");

    let grams = mcp_call_router_with_token(
        &app.router,
        22,
        "create_profile_log",
        json!({
            "profile_id":profile_id,
            "log": {
                "day":"2025-09-02",
                "entry":{"type":"food", "food_id":mass_food_id, "amount":"250", "serving_size":"g"}
            }
        }),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(grams.json["result"]["isError"], false);
    assert_eq!(
        grams.json["result"]["structuredContent"]["data"]["quantity"],
        "250 g"
    );
    assert_calories(
        &grams.json["result"]["structuredContent"]["data"]["nutrition"]["calories"],
        "500 kcal",
    )?;
    assert_mass(
        &grams.json["result"]["structuredContent"]["data"]["nutrition"]["protein"],
        "25 g",
    )?;

    let millilitres = mcp_call_router_with_token(
        &app.router,
        23,
        "create_profile_log",
        json!({
            "profile_id":profile_id,
            "log": {
                "day":"2025-09-02",
                "entry":{"type":"food", "food_id":food_id, "amount":"300", "serving_size":"ml"}
            }
        }),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(millilitres.json["result"]["isError"], false);
    assert_eq!(
        millilitres.json["result"]["structuredContent"]["data"]["quantity"],
        "300 ml"
    );
    assert_calories(
        &millilitres.json["result"]["structuredContent"]["data"]["nutrition"]["calories"],
        "150 kcal",
    )?;
    assert_mass(
        &millilitres.json["result"]["structuredContent"]["data"]["nutrition"]["protein"],
        "6 g",
    )?;

    let list = mcp_call_router_with_token(
        &app.router,
        24,
        "list_profile_logs",
        json!({"profile_id":profile_id, "filter":{"entry_type":"food"}}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(list.json["result"]["isError"], false);
    let items = list.json["result"]["structuredContent"]["items"]
        .as_array()
        .ok_or("MCP food list has no items")?;
    assert_eq!(items.len(), 3);

    let forged = mcp_call_router_with_token(
        &app.router,
        25,
        "create_profile_log",
        json!({
            "profile_id":profile_id,
            "log": {
                "day":"2025-09-03",
                "entry":{"type":"food", "food_id":food_id, "amount":"1", "serving_size":"ml", "description":"forged"}
            }
        }),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(forged.json["result"]["isError"], true);
    assert_eq!(
        forged.json["result"]["structuredContent"]["code"],
        "invalid_input"
    );

    let missing_calories = mcp_call_router_with_token(
        &app.router,
        26,
        "create_profile_log",
        json!({
            "profile_id":profile_id,
            "log": {
                "day":"2025-09-03",
                "entry":{"type":"food", "food_id":no_calories_id, "amount":"1", "serving_size":"ml"}
            }
        }),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(missing_calories.json["result"]["isError"], true);
    assert_eq!(
        missing_calories.json["result"]["structuredContent"]["code"],
        "invalid_input"
    );
    let calorie_message = missing_calories.json["result"]["structuredContent"]["error"]
        .as_str()
        .ok_or("missing-calorie MCP error has no message")?
        .to_lowercase();
    assert!(calorie_message.contains("calor"));
    assert!(calorie_message.contains("edit") || calorie_message.contains("create"));

    let updated = mcp_call_router_with_token(
        &app.router,
        27,
        "update_profile_log",
        json!({
            "profile_id":profile_id,
            "id":log_id,
            "patch":{"entry":{"type":"food", "amount":"3"}}
        }),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(updated.json["result"]["isError"], false);
    assert_eq!(
        updated.json["result"]["structuredContent"]["data"]["quantity"],
        "750 ml"
    );
    assert_calories(
        &updated.json["result"]["structuredContent"]["data"]["nutrition"]["calories"],
        "375 kcal",
    )?;

    let null_patch = mcp_call_router_with_token(
        &app.router,
        28,
        "update_profile_log",
        json!({
            "profile_id":profile_id,
            "id":log_id,
            "patch":{"entry":{"type":"food", "amount":null}}
        }),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(null_patch.json["result"]["isError"], true);
    assert_eq!(
        null_patch.json["result"]["structuredContent"]["code"],
        "invalid_input"
    );

    let profile_after_food = app.request(http_request(&profile_uri)?).await?;
    assert_eq!(
        profile_after_food.json["data"]["cache"]["current_weight"],
        "65 kg"
    );
    Ok(())
}

#[tokio::test]
async fn journal_mcp_round_trip_and_filter() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    app.seed_profile_owner().await?;
    let (profile_id, _) =
        create_test_profile(&app, "Journal", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    mcp_initialize(&app).await?;
    let original = "  café\n水  ";
    let created = mcp_call_router_with_token(&app.router, 1, "create_profile_log",
        json!({"profile_id":profile_id,"log":{"day":"2025-06-01","entry":{"type":"journal","text":original}}}),
        TEST_MCP_WRITE_ACCESS_TOKEN).await?;
    assert_eq!(
        created.json["result"]["isError"], false,
        "{:?}",
        created.json
    );
    let log = &created.json["result"]["structuredContent"];
    let id = log["id"].as_i64().ok_or("journal ID is missing")?;
    assert_eq!(log["data"]["text"], original);
    let blank = mcp_call_router_with_token(&app.router, 2, "create_profile_log",
        json!({"profile_id":profile_id,"log":{"day":"2025-06-01","entry":{"type":"journal","text":"  "}}}),
        TEST_MCP_WRITE_ACCESS_TOKEN).await?;
    assert_eq!(blank.json["result"]["isError"], true);
    let fetched = mcp_call_router_with_token(
        &app.router,
        3,
        "get_profile_log",
        json!({"profile_id":profile_id,"id":id}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(
        fetched.json["result"]["structuredContent"]["data"]["text"],
        original
    );
    let listed = mcp_call_router_with_token(
        &app.router,
        4,
        "list_profile_logs",
        json!({"profile_id":profile_id,"filter":{"entry_type":"journal"}}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(
        listed.json["result"]["structuredContent"]["items"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let updated = mcp_call_router_with_token(&app.router, 5, "update_profile_log",
        json!({"profile_id":profile_id,"id":id,"patch":{"entry":{"type":"journal","text":"Changed"}}}),
        TEST_MCP_WRITE_ACCESS_TOKEN).await?;
    assert_eq!(
        updated.json["result"]["structuredContent"]["data"]["text"],
        "Changed"
    );
    let denied = mcp_call_router_with_token(
        &app.router,
        6,
        "delete_profile_log",
        json!({"profile_id":profile_id,"id":id}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(denied.json["result"]["isError"], true);
    let deleted = mcp_call_router_with_token(
        &app.router,
        7,
        "delete_profile_log",
        json!({"profile_id":profile_id,"id":id}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(
        deleted.json["result"]["isError"], false,
        "{:?}",
        deleted.json
    );
    let missing = mcp_call_router_with_token(
        &app.router,
        8,
        "get_profile_log",
        json!({"profile_id":profile_id,"id":id}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(missing.json["result"]["isError"], true);
    Ok(())
}

#[tokio::test]
async fn exercise_mcp_round_trip_and_filter() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    app.seed_profile_owner().await?;
    let (profile_id, _) =
        create_test_profile(&app, "Exercise", TEST_REST_WRITE_ACCESS_TOKEN).await?;
    mcp_initialize(&app).await?;
    let entry = json!({"type":"exercise", "exercise_type":"weights training", "intensity":"hard", "duration":"30.5 min", "calories_burned":"150 kcal", "notes":"Gym"});
    let created = mcp_call_router_with_token(
        &app.router,
        1,
        "create_profile_log",
        json!({"profile_id":profile_id, "log":{"day":"2025-06-01", "entry":entry}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(
        created.json["result"]["isError"], false,
        "{:?}",
        created.json
    );
    let log = &created.json["result"]["structuredContent"];
    let mut expected = entry.clone();
    expected["duration"] = json!("1830 s");
    assert_eq!(log["data"], expected);
    let id = log["id"].as_i64().ok_or("exercise ID is missing")?;
    let updated = mcp_call_router_with_token(&app.router, 2, "update_profile_log", json!({"profile_id":profile_id, "id":id, "patch":{"entry":{"type":"exercise", "intensity":"easy", "notes":null}}}), TEST_MCP_WRITE_ACCESS_TOKEN).await?;
    assert_eq!(
        updated.json["result"]["structuredContent"]["data"],
        json!({"type":"exercise", "exercise_type":"weights training", "intensity":"easy", "duration":"1830 s", "calories_burned":"150 kcal", "notes":null})
    );
    let listed = mcp_call_router_with_token(
        &app.router,
        3,
        "list_profile_logs",
        json!({"profile_id":profile_id, "filter":{"entry_type":"exercise"}}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(
        listed.json["result"]["structuredContent"]["items"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    Ok(())
}
