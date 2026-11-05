use std::error::Error;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use crumbs_database::nutrition::FoodData;
use crumbs_database::{
    Database, DatabaseConfig, Dataset, Food, FoodId, FoodReader, FoodSearchResult, FoodWriter,
    SearchError, SearchLimit, SearchQuery, Source, TagName,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

use super::{config, search::NAME};
use crate::oauth::tests::{
    TEST_MCP_ACCESS_TOKEN, TEST_REST_ACCESS_TOKEN, test_authorization_server,
};

type SourceFoodIdRef<'a> = &'a str;
type DatasetRef = &'static str;
type FoodDescriptionRef<'a> = &'a str;

struct TestApplication {
    _directory: TempDir,
    database: Arc<Database>,
    _authorization_database: Arc<Database>,
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
        let authorization_database = Arc::new(
            Database::open(DatabaseConfig {
                path: directory.path().join("authorization.db"),
            })
            .await?,
        );
        let authorization = test_authorization_server(Arc::clone(&authorization_database)).await?;
        let router = crate::routes::application(Arc::clone(&database), config(), authorization);
        Ok(Self {
            _directory: directory,
            database,
            _authorization_database: authorization_database,
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
                dataset: Dataset::Borrowed("ingredient"),
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
        request_router(&self.router, request).await
    }
}

async fn request_router(
    router: &Router,
    request: Request<Body>,
) -> Result<ResponseBody, Box<dyn Error>> {
    let response = router.clone().oneshot(request).await?;
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

struct ResponseBody {
    status: StatusCode,
    headers: HeaderMap,
    json: Value,
}

fn http_request(uri: &str) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::builder()
        .uri(uri)
        .header(header::HOST, "foods.example")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {TEST_REST_ACCESS_TOKEN}"),
        )
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
    builder = builder.header(
        header::AUTHORIZATION,
        format!("Bearer {TEST_MCP_ACCESS_TOKEN}"),
    );
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
        .header(
            header::AUTHORIZATION,
            format!("Bearer {TEST_MCP_ACCESS_TOKEN}"),
        )
        .header("MCP-Protocol-Version", "2025-11-25")
        .body(Body::from(serde_json::to_vec(&body)?))?)
}

async fn mcp_initialize(app: &TestApplication) -> Result<(), Box<dyn Error>> {
    mcp_initialize_router(&app.router).await
}

async fn mcp_initialize_router(router: &Router) -> Result<(), Box<dyn Error>> {
    let response = request_router(
        router,
        mcp_request(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "transport-test", "version": "1" }
            }
        }))?,
    )
    .await?;
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.json);
    assert_eq!(response.json["result"]["capabilities"]["tools"], json!({}));

    let initialized = router
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
    mcp_call_router(&app.router, id, name, arguments).await
}

async fn mcp_call_router(
    router: &Router,
    id: u64,
    name: &str,
    arguments: Value,
) -> Result<ResponseBody, Box<dyn Error>> {
    request_router(
        router,
        protocol_request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }))?,
    )
    .await
}

async fn add_usda_food(
    database: &Database,
    source_food_id: SourceFoodIdRef<'_>,
    dataset: DatasetRef,
    description: FoodDescriptionRef<'_>,
) -> Result<(), Box<dyn Error>> {
    database
        .upsert_food(&Food {
            source: Source::Borrowed("usda"),
            source_food_id: source_food_id.to_owned(),
            dataset: Dataset::Borrowed(dataset),
            description: description.to_owned(),
            category: None,
            parsed_data: FoodData::default(),
        })
        .await?;
    Ok(())
}

#[tokio::test]
async fn http_and_mcp_use_food_reader_and_map_its_results_and_errors() -> Result<(), Box<dyn Error>>
{
    let reader = Arc::new(RecordingFoodReader::default());
    let directory = tempfile::tempdir()?;
    let authorization_database = Arc::new(
        Database::open(DatabaseConfig {
            path: directory.path().join("authorization.db"),
        })
        .await?,
    );
    let authorization = test_authorization_server(Arc::clone(&authorization_database)).await?;
    let router = crate::routes::application(Arc::clone(&reader), config(), authorization);

    let unauthorized_mcp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"unauthenticated","version":"1"}}}"#,
                ))?,
        )
        .await?;
    assert_eq!(unauthorized_mcp.status(), StatusCode::UNAUTHORIZED);
    assert!(reader.invocations()?.is_empty());

    let http = request_router(&router, http_request("/foods/search?query=beans&limit=1")?).await?;
    assert_eq!(http.status, StatusCode::OK, "{:?}", http.json);
    assert_eq!(http.json["foods"][0]["description"], "Test lentils");
    assert_eq!(
        http.json["foods"][0]["parsed_data"]["nutrition"]["energy"],
        "120 kcal"
    );
    assert_eq!(
        http.json["foods"][0]["parsed_data"]["nutrition"]["protein"],
        "8 g"
    );
    assert_eq!(
        reader.invocations()?,
        vec![SearchInvocation {
            query: SearchQuery::new("beans")?,
            limit: SearchLimit::from(1),
        }]
    );

    mcp_initialize_router(&router).await?;
    let mcp = mcp_call_router(&router, 2, NAME, json!({"query":"lentils", "limit":1001})).await?;
    assert_eq!(mcp.status, StatusCode::OK, "{:?}", mcp.json);
    assert_eq!(mcp.json["result"]["isError"], false);
    let mcp_result = &mcp.json["result"]["structuredContent"];
    assert_eq!(mcp_result["foods"][0]["description"], "Test lentils");
    assert_eq!(
        mcp_result["foods"][0]["parsed_data"]["nutrition"]["energy"],
        "120 kcal"
    );
    assert_eq!(
        reader.invocations()?,
        vec![
            SearchInvocation {
                query: SearchQuery::new("beans")?,
                limit: SearchLimit::from(1),
            },
            SearchInvocation {
                query: SearchQuery::new("lentils")?,
                limit: SearchLimit::from(1_001),
            },
        ]
    );

    reader.set_outcome(ReaderOutcome::InvalidQuery)?;
    let http_invalid =
        request_router(&router, http_request("/foods/search?query=invalid")?).await?;
    assert_eq!(http_invalid.status, StatusCode::BAD_REQUEST);
    assert_eq!(http_invalid.json["error"], "invalid FTS query from reader");
    let mcp_invalid = mcp_call_router(&router, 3, NAME, json!({"query":"invalid"})).await?;
    assert_eq!(mcp_invalid.status, StatusCode::OK);
    assert_eq!(mcp_invalid.json["result"]["isError"], true);
    assert_eq!(
        mcp_invalid.json["result"]["structuredContent"]["error"],
        "invalid FTS query from reader"
    );

    reader.set_outcome(ReaderOutcome::Storage)?;
    let http_storage =
        request_router(&router, http_request("/foods/search?query=storage")?).await?;
    assert_eq!(http_storage.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(http_storage.json["error"], "food search failed");
    assert!(
        !http_storage
            .json
            .to_string()
            .contains("private storage detail")
    );
    let mcp_storage = mcp_call_router(&router, 4, NAME, json!({"query":"storage"})).await?;
    assert_eq!(mcp_storage.status, StatusCode::OK);
    assert_eq!(mcp_storage.json["result"]["isError"], true);
    assert_eq!(
        mcp_storage.json["result"]["structuredContent"]["error"],
        "food search failed"
    );
    assert!(
        !mcp_storage
            .json
            .to_string()
            .contains("private storage detail")
    );
    Ok(())
}

#[tokio::test]
async fn mcp_search_supports_source_and_namespaced_dataset_filters() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    add_usda_food(
        &app.database,
        "foundation-1",
        "usda_foundation",
        "Chicken breast, raw",
    )
    .await?;
    add_usda_food(&app.database, "branded-1", "usda_branded", "Chicken broth").await?;

    mcp_initialize(&app).await?;
    let foundation = mcp_call(
        &app,
        2,
        NAME,
        json!({"query":"chicken dataset:usda_foundation"}),
    )
    .await?;
    assert_eq!(foundation.status, StatusCode::OK, "{:?}", foundation.json);
    assert_eq!(foundation.json["result"]["isError"], false);
    assert_eq!(
        foundation.json["result"]["structuredContent"]["foods"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        foundation.json["result"]["structuredContent"]["foods"][0]["description"],
        "Chicken breast, raw"
    );

    let usda = mcp_call(&app, 3, NAME, json!({"query":"chicken source:usda"})).await?;
    assert_eq!(usda.status, StatusCode::OK, "{:?}", usda.json);
    assert_eq!(usda.json["result"]["isError"], false);
    assert_eq!(
        usda.json["result"]["structuredContent"]["foods"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReaderOutcome {
    Success,
    InvalidQuery,
    Storage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchInvocation {
    query: SearchQuery,
    limit: SearchLimit,
}

struct RecordingFoodReader {
    invocations: Mutex<Vec<SearchInvocation>>,
    outcome: Mutex<ReaderOutcome>,
}

impl Default for RecordingFoodReader {
    fn default() -> Self {
        Self {
            invocations: Mutex::new(Vec::new()),
            outcome: Mutex::new(ReaderOutcome::Success),
        }
    }
}

impl RecordingFoodReader {
    fn set_outcome(&self, outcome: ReaderOutcome) -> Result<(), Box<dyn Error>> {
        *self
            .outcome
            .lock()
            .map_err(|_| io::Error::other("recording reader outcome lock was poisoned"))? = outcome;
        Ok(())
    }

    fn invocations(&self) -> Result<Vec<SearchInvocation>, Box<dyn Error>> {
        Ok(self
            .invocations
            .lock()
            .map_err(|_| io::Error::other("recording reader invocation lock was poisoned"))?
            .clone())
    }
}

impl FoodReader for RecordingFoodReader {
    type Error = io::Error;

    async fn search_foods(
        &self,
        query: &SearchQuery,
        limit: SearchLimit,
    ) -> Result<Vec<FoodSearchResult>, SearchError<Self::Error>> {
        self.invocations
            .lock()
            .map_err(|_| {
                SearchError::Storage(io::Error::other("recording reader lock was poisoned"))
            })?
            .push(SearchInvocation {
                query: query.clone(),
                limit,
            });
        let outcome = *self.outcome.lock().map_err(|_| {
            SearchError::Storage(io::Error::other("recording reader lock was poisoned"))
        })?;
        match outcome {
            ReaderOutcome::Success => {
                let parsed_data = serde_json::from_value(json!({
                    "nutrition": {
                        "per": "100 g",
                        "energy": "120 kcal",
                        "protein": "8 g"
                    },
                    "serving_sizes": []
                }))
                .map_err(|error| SearchError::Storage(io::Error::other(error)))?;
                Ok(vec![FoodSearchResult {
                    id: FoodId(42),
                    food: Food {
                        source: Source::Borrowed("recording"),
                        source_food_id: "fixture-42".to_owned(),
                        dataset: Dataset::Borrowed("ingredient"),
                        description: "Test lentils".to_owned(),
                        category: Some("Legumes".to_owned()),
                        parsed_data,
                    },
                }])
            }
            ReaderOutcome::InvalidQuery => Err(SearchError::InvalidQuery(
                "invalid FTS query from reader".to_owned(),
            )),
            ReaderOutcome::Storage => Err(SearchError::Storage(io::Error::other(
                "private storage detail",
            ))),
        }
    }
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
    assert_eq!(tool["name"], NAME);
    assert_eq!(tool["inputSchema"]["required"], json!(["query"]));
    assert_eq!(tool["inputSchema"]["properties"]["limit"]["default"], 20);
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    let description = tool["description"]
        .as_str()
        .ok_or("MCP search tool has no description")?;
    for guidance in [
        "descriptions, categories, tags, source, and dataset",
        "No matches return an empty foods list",
        "Invalid search expressions return an error",
    ] {
        assert!(description.contains(guidance), "missing {guidance:?}");
    }
    let query_description = tool["inputSchema"]["properties"]["query"]["description"]
        .as_str()
        .ok_or("MCP search query has no description")?;
    assert!(query_description.contains('\n'));
    for guidance in [
        "Syntax:",
        "Search fields:",
        "Examples:",
        "`AND`, `OR`, `NOT`",
        "case-insensitive",
        "description:chicken",
        "category:dairy",
        "tags:organic",
        "source:usda",
        "dataset:usda_foundation",
        "chicken dataset:usda_foundation",
        "dataset:usda_branded",
        "dataset:usda_sr_legacy",
        "dataset:usda_survey_fndds",
        "source values are stored as `usda`",
        "values requiring quotes, such as values containing spaces, hyphens, or slashes",
        "empty foods list",
        "Invalid expressions return an error.",
    ] {
        assert!(query_description.contains(guidance), "missing {guidance:?}");
    }
    let limit_description = tool["inputSchema"]["properties"]["limit"]["description"]
        .as_str()
        .ok_or("MCP search limit has no description")?;
    assert!(limit_description.contains("20 through 1000"));

    let mcp = mcp_call(&app, 3, NAME, json!({"query":"tags:vegan"})).await?;
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
        let response = mcp_call(&app, 2, NAME, json!({"query":query})).await?;
        assert_eq!(response.status, StatusCode::OK, "{:?}", response.json);
        assert_eq!(response.json["result"]["isError"], true);
        assert!(response.json["result"]["structuredContent"]["error"].is_string());
    }
    let no_match = mcp_call(&app, 3, NAME, json!({"query":"chicken"})).await?;
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
    let mcp = mcp_call(&app, 2, NAME, json!({"query":"chicken"})).await?;
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
    let mcp_default = mcp_call(&app, 2, NAME, json!({"query":"chicken"})).await?;
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
    let mcp_lower = mcp_call(&app, 3, NAME, json!({"query":"chicken", "limit":1})).await?;
    assert_eq!(mcp_lower.status, StatusCode::OK);
    assert_eq!(mcp_lower.json["result"]["structuredContent"], lower.json);
    assert_eq!(
        mcp_lower.json["result"]["structuredContent"]["foods"]
            .as_array()
            .map(Vec::len),
        Some(20)
    );
    let mcp = mcp_call(&app, 4, NAME, json!({"query":"chicken", "limit":1001})).await?;
    assert_eq!(mcp.status, StatusCode::OK);
    assert_eq!(
        mcp.json["result"]["structuredContent"]["foods"]
            .as_array()
            .map(Vec::len),
        Some(1_000)
    );
    Ok(())
}
