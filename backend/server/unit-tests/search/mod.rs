use std::error::Error;
use std::io;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use crumbs_database::foods::{
    CustomFoodInput, CustomFoodStore, Dataset, Food, FoodId, FoodLimit, FoodSearch, FoodWriter,
    SearchError, SearchQuery, SearchResults, Source, StoredFood, TagName, TagStore,
};
use crumbs_database::nutrition_model::FoodData;
use crumbs_database::{Database, DatabaseConfig};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::mcp::config;
use crate::oauth::tests::test_authorization_server;
use crate::tests::{
    TestApplication, http_request, mcp_call, mcp_call_router, mcp_initialize,
    mcp_initialize_router, protocol_request, request_router,
};

const NAME: &str = "search_foods";

impl TestApplication {
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
                dataset: Dataset::Common,
                description: description.to_owned(),
                category: Some("Protein".to_owned()),
                notes: None,
                parsed_data,
            })
            .await?;
        self.database
            .set_food_tags(food_id, &[TagName::new("vegan")?])
            .await?;
        Ok(())
    }
}

async fn add_usda_food(
    database: &Database,
    source_food_id: &str,
    dataset: Dataset,
    description: &str,
) -> Result<(), Box<dyn Error>> {
    database
        .upsert_food(&Food {
            source: Source::Borrowed("usda"),
            source_food_id: source_food_id.to_owned(),
            dataset,
            description: description.to_owned(),
            category: None,
            notes: None,
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
    assert_eq!(http.json["common"][0]["description"], "Test lentils");
    assert_eq!(
        http.json["common"][0]["parsed_data"]["nutrition"]["energy"],
        "120 kcal"
    );
    assert_eq!(
        http.json["common"][0]["parsed_data"]["nutrition"]["protein"],
        "8 g"
    );
    assert_eq!(
        reader.invocations()?,
        vec![SearchInvocation {
            query: SearchQuery::new("beans")?,
            limit: FoodLimit::from(1),
        }]
    );

    mcp_initialize_router(&router).await?;
    let mcp = mcp_call_router(&router, 2, NAME, json!({"query":"lentils", "limit":1001})).await?;
    assert_eq!(mcp.status, StatusCode::OK, "{:?}", mcp.json);
    assert_eq!(mcp.json["result"]["isError"], false);
    let mcp_result = &mcp.json["result"]["structuredContent"];
    assert_eq!(mcp_result["common"][0]["description"], "Test lentils");
    assert_eq!(
        mcp_result["common"][0]["parsed_data"]["nutrition"]["energy"],
        "120 kcal"
    );
    assert_eq!(
        reader.invocations()?,
        vec![
            SearchInvocation {
                query: SearchQuery::new("beans")?,
                limit: FoodLimit::from(1),
            },
            SearchInvocation {
                query: SearchQuery::new("lentils")?,
                limit: FoodLimit::from(1_001),
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
async fn mcp_search_supports_source_and_shared_dataset_filters() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    add_usda_food(
        &app.database,
        "foundation-1",
        Dataset::Common,
        "Chicken breast, raw",
    )
    .await?;
    add_usda_food(
        &app.database,
        "branded-1",
        Dataset::Branded,
        "Chicken broth",
    )
    .await?;

    mcp_initialize(&app).await?;
    let common = mcp_call(&app, 2, NAME, json!({"query":"chicken dataset:common"})).await?;
    assert_eq!(common.status, StatusCode::OK, "{:?}", common.json);
    assert_eq!(common.json["result"]["isError"], false);
    assert_eq!(
        common.json["result"]["structuredContent"]["common"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        common.json["result"]["structuredContent"]["common"][0]["description"],
        "Chicken breast, raw"
    );

    let usda = mcp_call(&app, 3, NAME, json!({"query":"chicken source:usda"})).await?;
    assert_eq!(usda.status, StatusCode::OK, "{:?}", usda.json);
    assert_eq!(usda.json["result"]["isError"], false);
    assert_eq!(
        usda.json["result"]["structuredContent"]["common"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        usda.json["result"]["structuredContent"]["branded"][0]["description"],
        "Chicken broth"
    );
    Ok(())
}
#[tokio::test]
async fn http_and_mcp_search_keep_independent_dataset_limits() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    for index in 0..21 {
        add_usda_food(
            &app.database,
            &format!("common-{index}"),
            Dataset::Common,
            &format!("Apricot common food {index}"),
        )
        .await?;
        add_usda_food(
            &app.database,
            &format!("branded-{index}"),
            Dataset::Branded,
            &format!("Apricot branded product {index}"),
        )
        .await?;
    }
    app.database
        .create_custom_food(&serde_json::from_value::<CustomFoodInput>(json!({
            "description": "Apricot custom recipe",
            "notes": "Dried apricots and oats"
        }))?)
        .await?;

    let http = app
        .request(http_request("/foods/search?query=apricot&limit=20")?)
        .await?;
    assert_eq!(http.status, StatusCode::OK, "{:?}", http.json);
    assert_eq!(http.json["common"].as_array().map(Vec::len), Some(20));
    assert_eq!(http.json["branded"].as_array().map(Vec::len), Some(20));
    assert_eq!(http.json["custom"].as_array().map(Vec::len), Some(1));

    let mcp = mcp_call(&app, 2, NAME, json!({"query":"apricot", "limit":20})).await?;
    assert_eq!(mcp.status, StatusCode::OK, "{:?}", mcp.json);
    assert_eq!(mcp.json["result"]["isError"], false);
    assert_eq!(mcp.json["result"]["structuredContent"], http.json);
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
    limit: FoodLimit,
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

impl FoodSearch for RecordingFoodReader {
    type Error = io::Error;

    async fn search_foods(
        &self,
        query: &SearchQuery,
        limit: FoodLimit,
    ) -> Result<SearchResults, SearchError<Self::Error>> {
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
                Ok(SearchResults {
                    common: vec![StoredFood {
                        id: FoodId(42),
                        food: Food {
                            source: Source::Borrowed("recording"),
                            source_food_id: "fixture-42".to_owned(),
                            dataset: Dataset::Common,
                            description: "Test lentils".to_owned(),
                            category: Some("Legumes".to_owned()),
                            notes: Some("Lentils, water".to_owned()),
                            parsed_data,
                        },
                    }],
                    ..SearchResults::default()
                })
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

impl CustomFoodStore for RecordingFoodReader {
    type Error = io::Error;

    async fn create_custom_food(
        &self,
        _input: &CustomFoodInput,
    ) -> Result<StoredFood, Self::Error> {
        Err(io::Error::other(
            "management unavailable in recording reader",
        ))
    }

    async fn custom_food(&self, _id: FoodId) -> Result<Option<StoredFood>, Self::Error> {
        Err(io::Error::other(
            "management unavailable in recording reader",
        ))
    }

    async fn list_custom_foods(&self, _limit: FoodLimit) -> Result<Vec<StoredFood>, Self::Error> {
        Err(io::Error::other(
            "management unavailable in recording reader",
        ))
    }

    async fn update_custom_food(
        &self,
        _id: FoodId,
        _input: &CustomFoodInput,
    ) -> Result<Option<StoredFood>, Self::Error> {
        Err(io::Error::other(
            "management unavailable in recording reader",
        ))
    }

    async fn delete_custom_food(&self, _id: FoodId) -> Result<bool, Self::Error> {
        Err(io::Error::other(
            "management unavailable in recording reader",
        ))
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
    assert_eq!(http.json["common"].as_array().map(Vec::len), Some(1));
    assert_eq!(http.json["common"][0]["description"], "Chickpeas");
    assert_eq!(
        http.json["common"][0]["parsed_data"]["nutrition"]["energy"],
        "210 kcal"
    );
    assert_eq!(
        http.json["common"][0]["parsed_data"]["nutrition"]["protein"],
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
    let tool = tools.json["result"]["tools"]
        .as_array()
        .and_then(|items| items.iter().find(|tool| tool["name"] == NAME))
        .cloned()
        .ok_or("MCP search tool is missing")?;
    assert_eq!(tool["name"], NAME);
    assert_eq!(tool["inputSchema"]["required"], json!(["query"]));
    assert_eq!(tool["inputSchema"]["properties"]["limit"]["default"], 20);
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    let description = tool["description"]
        .as_str()
        .ok_or("MCP search tool has no description")?;
    for guidance in [
        "common, branded, and custom lists",
        "independent limits",
        "optional notes",
        "Choose the appropriate food across datasets",
        "No matches return three empty lists",
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
        "dataset:common",
        "chicken dataset:common",
        "dataset:branded",
        "dataset:custom",
        "USDA source values are stored as `usda`",
        "values requiring quotes, such as values containing spaces, hyphens, or slashes",
        "three empty lists",
        "Food notes are returned but are not searched",
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
    assert_eq!(
        no_match.json,
        json!({ "common": [], "branded": [], "custom": [] })
    );
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
        json!({"common": [], "branded": [], "custom": []})
    );
    assert_eq!(no_match.json["result"]["isError"], false);
    let unknown = mcp_call(&app, 4, "unknown", json!({})).await?;
    assert_eq!(unknown.status, StatusCode::OK);
    assert_eq!(unknown.json["error"]["code"], -32601);
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
    assert_eq!(default.json["common"].as_array().map(Vec::len), Some(20));
    assert!(
        default.json["branded"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert!(default.json["custom"].as_array().is_some_and(Vec::is_empty));
    let lower = app
        .request(http_request("/foods/search?query=chicken&limit=1")?)
        .await?;
    assert_eq!(lower.status, StatusCode::OK);
    assert_eq!(lower.json["common"].as_array().map(Vec::len), Some(20));
    let capped = app
        .request(http_request("/foods/search?query=chicken&limit=1001")?)
        .await?;
    assert_eq!(capped.status, StatusCode::OK);
    assert_eq!(capped.json["common"].as_array().map(Vec::len), Some(1_000));
    mcp_initialize(&app).await?;
    let mcp_default = mcp_call(&app, 2, NAME, json!({"query":"chicken"})).await?;
    assert_eq!(mcp_default.status, StatusCode::OK);
    assert_eq!(
        mcp_default.json["result"]["structuredContent"],
        default.json
    );
    assert_eq!(
        mcp_default.json["result"]["structuredContent"]["common"]
            .as_array()
            .map(Vec::len),
        Some(20)
    );
    let mcp_lower = mcp_call(&app, 3, NAME, json!({"query":"chicken", "limit":1})).await?;
    assert_eq!(mcp_lower.status, StatusCode::OK);
    assert_eq!(mcp_lower.json["result"]["structuredContent"], lower.json);
    assert_eq!(
        mcp_lower.json["result"]["structuredContent"]["common"]
            .as_array()
            .map(Vec::len),
        Some(20)
    );
    let mcp = mcp_call(&app, 4, NAME, json!({"query":"chicken", "limit":1001})).await?;
    assert_eq!(mcp.status, StatusCode::OK);
    assert_eq!(
        mcp.json["result"]["structuredContent"]["common"]
            .as_array()
            .map(Vec::len),
        Some(1_000)
    );
    Ok(())
}
