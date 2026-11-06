use std::error::Error;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use crumbs_database::{Database, DatabaseConfig};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

use crate::mcp::config;
use crate::oauth::tests::{
    TEST_ISSUER, TEST_MCP_ACCESS_TOKEN, TEST_REST_ACCESS_TOKEN, test_authorization_server,
};

pub(crate) struct TestApplication {
    _directory: TempDir,
    pub(crate) database: Arc<Database>,
    pub(crate) _authorization_database: Arc<Database>,
    pub(crate) router: Router,
}

impl TestApplication {
    pub(crate) async fn new() -> Result<Self, Box<dyn Error>> {
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

    pub(crate) async fn request(
        &self,
        request: Request<Body>,
    ) -> Result<ResponseBody, Box<dyn Error>> {
        request_router(&self.router, request).await
    }
}

pub(crate) async fn request_router(
    router: &Router,
    request: Request<Body>,
) -> Result<ResponseBody, Box<dyn Error>> {
    let response = router.clone().oneshot(request).await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let json = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body)?
    };
    Ok(ResponseBody {
        status,
        headers,
        json,
    })
}

pub(crate) struct ResponseBody {
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) json: Value,
}

pub(crate) fn assert_mcp_insufficient_scope_challenge(
    response: &ResponseBody,
    scope: &str,
) -> Result<(), Box<dyn Error>> {
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    let challenge = response
        .headers
        .get(header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .ok_or("MCP scope denial has no WWW-Authenticate challenge")?;
    let metadata = format!("{TEST_ISSUER}/.well-known/oauth-protected-resource/mcp");
    assert!(challenge.starts_with("Bearer "), "{challenge}");
    assert!(
        challenge.contains("error=\"insufficient_scope\""),
        "{challenge}"
    );
    assert!(
        challenge.contains(&format!("scope=\"{scope}\"")),
        "{challenge}"
    );
    assert!(
        challenge.contains(&format!("resource_metadata=\"{metadata}\"")),
        "{challenge}"
    );
    Ok(())
}

pub(crate) fn http_request(uri: &str) -> Result<Request<Body>, Box<dyn Error>> {
    http_request_with_token(uri, TEST_REST_ACCESS_TOKEN)
}

pub(crate) fn http_request_with_token(
    uri: &str,
    token: &str,
) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::builder()
        .uri(uri)
        .header(header::HOST, "foods.example")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())?)
}

pub(crate) fn http_json_request(
    method: &str,
    uri: &str,
    token: &str,
    value: Value,
) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "foods.example")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&value)?))?)
}

pub(crate) fn http_method_request(
    method: &str,
    uri: &str,
    token: &str,
) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "foods.example")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())?)
}

pub(crate) fn mcp_request(body: Value) -> Result<Request<Body>, Box<dyn Error>> {
    mcp_request_with_origin(body, None)
}

pub(crate) fn mcp_request_with_origin(
    body: Value,
    origin: Option<&str>,
) -> Result<Request<Body>, Box<dyn Error>> {
    mcp_request_with_token_and_origin(body, TEST_MCP_ACCESS_TOKEN, origin)
}

pub(crate) fn mcp_request_with_token_and_origin(
    body: Value,
    token: &str,
    origin: Option<&str>,
) -> Result<Request<Body>, Box<dyn Error>> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "foods.example")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream");
    builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    Ok(builder.body(Body::from(serde_json::to_vec(&body)?))?)
}

pub(crate) fn protocol_request(body: Value) -> Result<Request<Body>, Box<dyn Error>> {
    protocol_request_with_token(body, TEST_MCP_ACCESS_TOKEN)
}

pub(crate) fn protocol_request_with_token(
    body: Value,
    token: &str,
) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "foods.example")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("MCP-Protocol-Version", "2025-11-25")
        .body(Body::from(serde_json::to_vec(&body)?))?)
}

pub(crate) async fn mcp_initialize(app: &TestApplication) -> Result<(), Box<dyn Error>> {
    mcp_initialize_router(&app.router).await
}

pub(crate) async fn mcp_initialize_router(router: &Router) -> Result<(), Box<dyn Error>> {
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

pub(crate) async fn mcp_call(
    app: &TestApplication,
    id: u64,
    name: &str,
    arguments: Value,
) -> Result<ResponseBody, Box<dyn Error>> {
    mcp_call_router(&app.router, id, name, arguments).await
}

pub(crate) async fn mcp_call_router(
    router: &Router,
    id: u64,
    name: &str,
    arguments: Value,
) -> Result<ResponseBody, Box<dyn Error>> {
    mcp_call_router_with_token(router, id, name, arguments, TEST_MCP_ACCESS_TOKEN).await
}

pub(crate) async fn mcp_call_router_with_token(
    router: &Router,
    id: u64,
    name: &str,
    arguments: Value,
    token: &str,
) -> Result<ResponseBody, Box<dyn Error>> {
    request_router(
        router,
        protocol_request_with_token(
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": { "name": name, "arguments": arguments }
            }),
            token,
        )?,
    )
    .await
}
