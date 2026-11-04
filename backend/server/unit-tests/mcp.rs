use std::error::Error;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use crumbs_database::nutrition::FoodData;
use crumbs_database::{Database, DatabaseConfig, Food, FoodType, FoodWriter, Source, TagName};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

use super::{SEARCH_TOOL_NAME, config};

struct TestApplication {
    _directory: TempDir,
    database: Arc<Database>,
    router: Router,
}

impl TestApplication {
    async fn new() -> Result<Self, Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let database = Arc::new(
            Database::open(DatabaseConfig {
                path: directory.path().join("crumbs.db"),
            })
            .await?,
        );
        let router = crate::application(Arc::clone(&database), config());
        Ok(Self {
            _directory: directory,
            database,
            router,
        })
    }

    async fn add_food(
        &self,
        source_food_id: &str,
        description: &str,
    ) -> Result<(), Box<dyn Error>> {
        let parsed_data: FoodData = serde_json::from_value(json!({
            "nutrition": { "per": "100 g", "energy": "210 kcal", "protein": "7 g" },
            "serving_sizes": []
        }))?;
        let food_id = self
            .database
            .upsert_food(&Food {
                source: Source::Borrowed("test"),
                source_food_id: source_food_id.to_owned(),
                food_type: FoodType::Borrowed("ingredient"),
                description: description.to_owned(),
                category: Some("Protein".to_owned()),
                parsed_data,
            })
            .await?;
        self.database
            .set_food_tags(food_id, &[TagName::new("vegan")?])
            .await?;
        Ok(())
    }

    async fn request(&self, request: Request<Body>) -> Result<ResponseBody, Box<dyn Error>> {
        let response = self.router.clone().oneshot(request).await?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let json = serde_json::from_slice(&body)?;
        Ok(ResponseBody {
            status,
            headers,
            json,
        })
    }
}

struct ResponseBody {
    status: StatusCode,
    headers: HeaderMap,
    json: Value,
}

fn http_request(uri: &str) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::builder()
        .uri(uri)
        .header(header::HOST, "foods.example")
        .body(Body::empty())?)
}

fn mcp_request(body: Value) -> Result<Request<Body>, Box<dyn Error>> {
    mcp_request_with_origin(body, None)
}

fn mcp_request_with_origin(
    body: Value,
    origin: Option<&str>,
) -> Result<Request<Body>, Box<dyn Error>> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "foods.example")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream");
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    Ok(builder.body(Body::from(serde_json::to_vec(&body)?))?)
}

fn protocol_request(body: Value) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "foods.example")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-11-25")
        .body(Body::from(serde_json::to_vec(&body)?))?)
}

async fn mcp_initialize(app: &TestApplication) -> Result<(), Box<dyn Error>> {
    let response = app
        .request(mcp_request(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "transport-test", "version": "1" }
            }
        }))?)
        .await?;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.json);
    assert_eq!(response.json["result"]["capabilities"]["tools"], json!({}));

    let initialized = app
        .router
        .clone()
        .oneshot(protocol_request(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))?)
        .await?;
    assert_eq!(initialized.status(), StatusCode::ACCEPTED);
    Ok(())
}

async fn mcp_call(
    app: &TestApplication,
    id: u64,
    name: &str,
    arguments: Value,
) -> Result<ResponseBody, Box<dyn Error>> {
    app.request(protocol_request(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    }))?)
    .await
}

#[tokio::test]
async fn http_and_mcp_return_the_same_food_and_full_nutrition() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    app.add_food("1", "Chickpeas").await?;

    let http = app
        .request(http_request("/foods/search?query=tags%3Avegan")?)
        .await?;
    assert_eq!(http.status, StatusCode::OK, "{:?}", http.json);
    assert_eq!(http.json["foods"].as_array().map(Vec::len), Some(1));
    assert_eq!(http.json["foods"][0]["description"], "Chickpeas");
    assert_eq!(
        http.json["foods"][0]["parsed_data"]["nutrition"]["energy"],
        "210 kcal"
    );
    assert_eq!(
        http.json["foods"][0]["parsed_data"]["nutrition"]["protein"],
        "7 g"
    );
    assert_eq!(
        http.headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json")
    );

    mcp_initialize(&app).await?;
    let tools = app
        .request(protocol_request(json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}
        }))?)
        .await?;
    assert_eq!(tools.status, StatusCode::OK, "{:?}", tools.json);
    let tool = tools.json["result"]["tools"][0].clone();
    assert_eq!(tool["name"], SEARCH_TOOL_NAME);
    assert_eq!(tool["inputSchema"]["required"], json!(["query"]));
    assert_eq!(tool["inputSchema"]["properties"]["limit"]["default"], 20);
    assert_eq!(tool["annotations"]["readOnlyHint"], true);

    let mcp = mcp_call(&app, 3, SEARCH_TOOL_NAME, json!({"query":"tags:vegan"})).await?;
    assert_eq!(mcp.status, StatusCode::OK, "{:?}", mcp.json);
    assert_eq!(mcp.json["result"]["structuredContent"], http.json);
    assert_eq!(mcp.json["result"]["isError"], false);
    let fallback = mcp.json["result"]["content"][0]["text"]
        .as_str()
        .ok_or("MCP tool result has no text fallback")?;
    assert_eq!(serde_json::from_str::<Value>(fallback)?, http.json);
    assert_eq!(mcp.headers.get("mcp-session-id"), None);
    Ok(())
}

#[tokio::test]
async fn http_rejects_invalid_input_and_returns_empty_matches() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    for uri in [
        "/foods/search",
        "/foods/search?query=%20%20",
        "/foods/search?query=%28",
    ] {
        let response = app.request(http_request(uri)?).await?;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{uri}: {:?}",
            response.json
        );
        assert!(response.json["error"].is_string());
    }
    let no_match = app
        .request(http_request("/foods/search?query=chicken")?)
        .await?;
    assert_eq!(no_match.status, StatusCode::OK);
    assert_eq!(no_match.json, json!({ "foods": [] }));
    Ok(())
}

#[tokio::test]
async fn mcp_returns_search_errors_as_tool_errors_and_unknown_tool_as_protocol_error()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    mcp_initialize(&app).await?;
    for query in ["", "   ", "("] {
        let response = mcp_call(&app, 2, SEARCH_TOOL_NAME, json!({"query":query})).await?;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.json);
        assert_eq!(response.json["result"]["isError"], true);
        assert!(response.json["result"]["structuredContent"]["error"].is_string());
    }
    let no_match = mcp_call(&app, 3, SEARCH_TOOL_NAME, json!({"query":"chicken"})).await?;
    assert_eq!(no_match.status, StatusCode::OK);
    assert_eq!(
        no_match.json["result"]["structuredContent"],
        json!({"foods": []})
    );
    assert_eq!(no_match.json["result"]["isError"], false);
    let unknown = mcp_call(&app, 4, "unknown", json!({})).await?;
    assert_eq!(unknown.status, StatusCode::OK);
    assert_eq!(unknown.json["error"]["code"], -32601);
    Ok(())
}

#[tokio::test]
async fn hosted_host_is_accepted_and_unapproved_browser_origin_is_rejected()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    let hosted = app.request(mcp_request_with_origin(json!({
        "jsonrpc":"2.0", "id":1, "method":"initialize",
        "params": {"protocolVersion":"2025-11-25", "capabilities":{}, "clientInfo":{"name":"hosted","version":"1"}}
    }), None)?).await?;
    assert_eq!(hosted.status, StatusCode::OK, "{:?}", hosted.json);
    let rejected = app
        .router
        .clone()
        .oneshot(mcp_request_with_origin(
            json!({"jsonrpc":"2.0","id":2,"method":"ping"}),
            Some("https://evil.example"),
        )?)
        .await?;
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    Ok(())
}

#[tokio::test]
async fn database_failures_are_hidden_from_http_and_mcp_callers() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    mcp_initialize(&app).await?;
    app.database.close().await;
    let http = app
        .request(http_request("/foods/search?query=chicken")?)
        .await?;
    assert_eq!(http.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(http.json["error"], "food search failed");
    let mcp = mcp_call(&app, 2, SEARCH_TOOL_NAME, json!({"query":"chicken"})).await?;
    assert_eq!(mcp.json["result"]["isError"], true);
    assert_eq!(
        mcp.json["result"]["structuredContent"]["error"],
        "food search failed"
    );
    Ok(())
}

#[tokio::test]
async fn limits_default_to_twenty_and_cap_at_one_thousand() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    for index in 0..1_001 {
        app.add_food(&index.to_string(), &format!("Chicken food {index}"))
            .await?;
    }
    let default = app
        .request(http_request("/foods/search?query=chicken")?)
        .await?;
    assert_eq!(default.status, StatusCode::OK);
    assert_eq!(default.json["foods"].as_array().map(Vec::len), Some(20));
    let lower = app
        .request(http_request("/foods/search?query=chicken&limit=1")?)
        .await?;
    assert_eq!(lower.status, StatusCode::OK);
    assert_eq!(lower.json["foods"].as_array().map(Vec::len), Some(20));
    let capped = app
        .request(http_request("/foods/search?query=chicken&limit=1001")?)
        .await?;
    assert_eq!(capped.status, StatusCode::OK);
    assert_eq!(capped.json["foods"].as_array().map(Vec::len), Some(1_000));
    mcp_initialize(&app).await?;
    let mcp_default = mcp_call(&app, 2, SEARCH_TOOL_NAME, json!({"query":"chicken"})).await?;
    assert_eq!(mcp_default.status, StatusCode::OK);
    assert_eq!(
        mcp_default.json["result"]["structuredContent"],
        default.json
    );
    assert_eq!(
        mcp_default.json["result"]["structuredContent"]["foods"]
            .as_array()
            .map(Vec::len),
        Some(20)
    );
    let mcp_lower = mcp_call(
        &app,
        3,
        SEARCH_TOOL_NAME,
        json!({"query":"chicken", "limit":1}),
    )
    .await?;
    assert_eq!(mcp_lower.status, StatusCode::OK);
    assert_eq!(mcp_lower.json["result"]["structuredContent"], lower.json);
    assert_eq!(
        mcp_lower.json["result"]["structuredContent"]["foods"]
            .as_array()
            .map(Vec::len),
        Some(20)
    );
    let mcp = mcp_call(
        &app,
        4,
        SEARCH_TOOL_NAME,
        json!({"query":"chicken", "limit":1001}),
    )
    .await?;
    assert_eq!(mcp.status, StatusCode::OK);
    assert_eq!(
        mcp.json["result"]["structuredContent"]["foods"]
            .as_array()
            .map(Vec::len),
        Some(1_000)
    );
    Ok(())
}
