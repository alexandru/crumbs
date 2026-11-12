use std::error::Error;

use axum::http::StatusCode;
use crumbs_database::foods::{Dataset, Food, FoodWriter, Source};
use crumbs_database::identity::OAuthScope;
use crumbs_database::nutrition_model::FoodData;
use serde_json::{Value, json};

use crate::oauth::tests::{
    TEST_MCP_ACCESS_TOKEN, TEST_MCP_RESOURCE, TEST_MCP_WRITE_ACCESS_TOKEN, seed_session_with_scope,
};
use crate::tests::{
    TestApplication, assert_mcp_insufficient_scope_challenge, mcp_call_router_with_token,
    mcp_initialize, protocol_request,
};

const TEST_MCP_WRITE_ONLY_ACCESS_TOKEN: &str = "test-mcp-write-only-access-token";
const TEST_MCP_WRITE_ONLY_REFRESH_TOKEN: &str = "test-mcp-write-only-refresh-token";

#[tokio::test]
async fn mcp_custom_food_tools_cover_crud_and_check_scope_per_request() -> Result<(), Box<dyn Error>>
{
    let app = TestApplication::new().await?;
    seed_session_with_scope(
        app._authorization_database.as_ref(),
        "test-mcp-write-only-client",
        TEST_MCP_RESOURCE,
        TEST_MCP_WRITE_ONLY_ACCESS_TOKEN,
        TEST_MCP_WRITE_ONLY_REFRESH_TOKEN,
        OAuthScope::Write,
    )
    .await?;
    mcp_initialize(&app).await?;

    let tools = app
        .request(protocol_request(json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}
        }))?)
        .await?;
    let tools = tools.json["result"]["tools"]
        .as_array()
        .ok_or("MCP tools list is missing")?;
    for name in [
        "list_custom_foods",
        "get_custom_food",
        "create_custom_food",
        "update_custom_food",
        "delete_custom_food",
    ] {
        assert!(
            tools.iter().any(|tool| tool["name"] == name),
            "missing {name}"
        );
    }
    let create_tool = tools
        .iter()
        .find(|tool| tool["name"] == "create_custom_food")
        .ok_or("MCP create tool is missing")?;
    assert_eq!(create_tool["annotations"]["readOnlyHint"], false);
    assert_eq!(create_tool["annotations"]["destructiveHint"], false);
    let update_tool = tools
        .iter()
        .find(|tool| tool["name"] == "update_custom_food")
        .ok_or("MCP update tool is missing")?;
    assert_eq!(update_tool["annotations"]["readOnlyHint"], false);
    assert_eq!(update_tool["annotations"]["destructiveHint"], true);
    let delete_tool = tools
        .iter()
        .find(|tool| tool["name"] == "delete_custom_food")
        .ok_or("MCP delete tool is missing")?;
    assert_eq!(delete_tool["annotations"]["destructiveHint"], true);

    let create_arguments = json!({
        "food": {
            "description": "MCP sesame noodles",
            "notes": "Noodles, sesame paste, lime",
            "parsed_data": {
                "nutrition": { "per": "100 g", "energy": "315 kcal", "protein": "9 g", "caffeine": "20 mg", "minerals": {"potassium": "100 mg"}, "vitamins": {"vitamin_b12": "1 µg"} },
                "serving_sizes": []
            }
        }
    });
    let read_denied_create = mcp_call_router_with_token(
        &app.router,
        3,
        "create_custom_food",
        create_arguments.clone(),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_mcp_insufficient_scope_challenge(&read_denied_create, "crumbs:write")?;
    assert_eq!(read_denied_create.json["result"]["isError"], true);
    assert_eq!(
        read_denied_create.json["result"]["structuredContent"],
        json!({"error":"insufficient_scope", "required_scope":"crumbs:write"})
    );

    let empty_after_denial = mcp_call_router_with_token(
        &app.router,
        16,
        "list_custom_foods",
        json!({}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(empty_after_denial.status, StatusCode::OK);
    assert_eq!(
        empty_after_denial.json["result"]["structuredContent"]["foods"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );

    let created = mcp_call_router_with_token(
        &app.router,
        3,
        "create_custom_food",
        create_arguments,
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(created.status, StatusCode::OK, "{:?}", created.json);
    assert_eq!(created.json["result"]["isError"], false);
    let created_food = &created.json["result"]["structuredContent"];
    assert_eq!(
        created_food["parsed_data"]["nutrition"]["caffeine"],
        "20 mg"
    );
    assert_eq!(
        created_food["parsed_data"]["nutrition"]["minerals"]["potassium"],
        "100 mg"
    );
    assert_eq!(
        created_food["parsed_data"]["nutrition"]["vitamins"]["vitamin_b12"],
        "1 µg"
    );
    let id = created_food["id"].as_i64().ok_or("MCP food has no ID")?;
    assert_eq!(created_food["source"], "custom");
    assert_eq!(created_food["dataset"], "custom");
    assert_eq!(created_food["notes"], "Noodles, sesame paste, lime");
    assert_eq!(
        created_food["parsed_data"]["nutrition"]["energy"],
        "315 kcal"
    );

    let fetched = mcp_call_router_with_token(
        &app.router,
        5,
        "get_custom_food",
        json!({"id":id}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(fetched.json["result"]["isError"], false);
    assert_eq!(fetched.json["result"]["structuredContent"], *created_food);
    let listed = mcp_call_router_with_token(
        &app.router,
        6,
        "list_custom_foods",
        json!({"limit":20}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(listed.json["result"]["isError"], false);
    assert_eq!(
        listed.json["result"]["structuredContent"]["foods"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        listed.json["result"]["structuredContent"]["foods"][0],
        *created_food
    );

    let write_only_read = mcp_call_router_with_token(
        &app.router,
        17,
        "list_custom_foods",
        json!({}),
        TEST_MCP_WRITE_ONLY_ACCESS_TOKEN,
    )
    .await?;
    assert_mcp_insufficient_scope_challenge(&write_only_read, "crumbs:read")?;
    assert_eq!(write_only_read.json["result"]["isError"], true);
    assert_eq!(
        write_only_read.json["result"]["structuredContent"],
        json!({"error":"insufficient_scope", "required_scope":"crumbs:read"})
    );

    let read_denied_update = mcp_call_router_with_token(
        &app.router,
        7,
        "update_custom_food",
        json!({"id":id, "food":{"description":"Denied update"}}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_mcp_insufficient_scope_challenge(&read_denied_update, "crumbs:write")?;
    assert_eq!(read_denied_update.json["result"]["isError"], true);
    assert_eq!(
        read_denied_update.json["result"]["structuredContent"]["required_scope"],
        "crumbs:write"
    );

    let updated = mcp_call_router_with_token(
        &app.router,
        8,
        "update_custom_food",
        json!({"id":id, "food":{"description":"MCP updated noodles"}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(updated.json["result"]["isError"], false);
    assert_eq!(
        updated.json["result"]["structuredContent"]["description"],
        "MCP updated noodles"
    );
    assert_eq!(
        updated.json["result"]["structuredContent"]["notes"],
        Value::Null
    );
    assert_eq!(
        updated.json["result"]["structuredContent"]["parsed_data"]["nutrition"],
        Value::Null
    );
    let visible = mcp_call_router_with_token(
        &app.router,
        9,
        "search_foods",
        json!({"query":"updated"}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(
        visible.json["result"]["structuredContent"]["custom"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let invalid = mcp_call_router_with_token(
        &app.router,
        10,
        "create_custom_food",
        json!({"food":{"description":"  "}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(invalid.json["result"]["isError"], true);
    assert!(invalid.json["result"]["structuredContent"]["error"].is_string());
    let missing = mcp_call_router_with_token(
        &app.router,
        11,
        "get_custom_food",
        json!({"id":999_999}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(missing.json["result"]["isError"], true);
    assert_eq!(
        missing.json["result"]["structuredContent"]["error"],
        "custom food not found"
    );

    let imported_id = app
        .database
        .upsert_food(&Food {
            source: Source::Borrowed("usda"),
            source_food_id: "mcp-protected-import".to_owned(),
            dataset: Dataset::Common,
            description: "Imported lentils".to_owned(),
            category: None,
            notes: None,
            parsed_data: FoodData::default(),
        })
        .await?;
    let imported_update = mcp_call_router_with_token(
        &app.router,
        12,
        "update_custom_food",
        json!({"id":imported_id.0, "food":{"description":"Hijacked lentils"}}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(imported_update.json["result"]["isError"], true);
    assert_eq!(
        imported_update.json["result"]["structuredContent"]["error"],
        "custom food not found"
    );
    let imported_delete = mcp_call_router_with_token(
        &app.router,
        13,
        "delete_custom_food",
        json!({"id":imported_id.0}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(imported_delete.json["result"]["isError"], true);
    assert_eq!(
        imported_delete.json["result"]["structuredContent"]["error"],
        "custom food not found"
    );

    let read_denied_delete = mcp_call_router_with_token(
        &app.router,
        14,
        "delete_custom_food",
        json!({"id":id}),
        TEST_MCP_ACCESS_TOKEN,
    )
    .await?;
    assert_mcp_insufficient_scope_challenge(&read_denied_delete, "crumbs:write")?;
    assert_eq!(read_denied_delete.json["result"]["isError"], true);
    assert_eq!(
        read_denied_delete.json["result"]["structuredContent"]["required_scope"],
        "crumbs:write"
    );
    let deleted = mcp_call_router_with_token(
        &app.router,
        15,
        "delete_custom_food",
        json!({"id":id}),
        TEST_MCP_WRITE_ACCESS_TOKEN,
    )
    .await?;
    assert_eq!(deleted.json["result"]["isError"], false);
    assert_eq!(
        deleted.json["result"]["structuredContent"],
        json!({"id":id, "deleted":true})
    );
    Ok(())
}
